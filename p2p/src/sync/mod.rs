// Copyright (c) 2022 RBB S.r.l
// opensource@mintlayer.org
// SPDX-License-Identifier: MIT
// Licensed under the MIT License;
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://github.com/mintlayer/mintlayer-core/blob/master/LICENSE
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This module is responsible for both initial syncing and further blocks processing (the reaction
//! to block announcement from peers and the announcement of blocks produced by this node).

// TODO: FIXME:
// mod peer_context;
mod peer;

use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use void::Void;

use chainstate::chainstate_interface::ChainstateInterface;
use common::{
    chain::{block::Block, config::ChainConfig},
    primitives::Id,
};
use logging::log;
use utils::tap_error_log::LogError;

use crate::{
    config::P2pConfig,
    error::{P2pError, PeerError},
    event::{PeerManagerEvent, SyncControlEvent},
    message::Announcement,
    net::{types::SyncingEvent, NetworkingService, SyncingMessagingService},
    sync::peer::Peer,
    types::peer_id::PeerId,
    Result,
};

/// Sync manager is responsible for syncing the local blockchain to the chain with most trust
/// and keeping up with updates to different branches of the blockchain.
pub struct BlockSyncManager<T: NetworkingService> {
    /// The chain configuration.
    _chain_config: Arc<ChainConfig>,

    /// The p2p configuration.
    p2p_config: Arc<P2pConfig>,

    /// A handle for sending/receiving syncing events.
    messaging_handle: T::SyncingMessagingHandle,

    /// A receiver for connect/disconnect events.
    peer_event_receiver: UnboundedReceiver<SyncControlEvent>,

    /// A sender for the peer manager events.
    peer_manager_sender: UnboundedSender<PeerManagerEvent<T>>,

    /// A handle to the chainstate subsystem.
    chainstate_handle: subsystem::Handle<Box<dyn ChainstateInterface>>,

    /// A cached result of the `ChainstateInterface::is_initial_block_download` call.
    is_initial_block_download: bool,

    /// A mapping from a peer identifier to the channel.
    peers: HashMap<PeerId, UnboundedSender<SyncingEvent>>,
}

/// Syncing manager
impl<T> BlockSyncManager<T>
where
    T: NetworkingService,
    T::SyncingMessagingHandle: SyncingMessagingService<T>,
{
    /// Creates a new sync manager instance.
    pub fn new(
        chain_config: Arc<ChainConfig>,
        p2p_config: Arc<P2pConfig>,
        messaging_handle: T::SyncingMessagingHandle,
        chainstate_handle: subsystem::Handle<Box<dyn ChainstateInterface>>,
        peer_event_receiver: UnboundedReceiver<SyncControlEvent>,
        peer_manager_sender: UnboundedSender<PeerManagerEvent<T>>,
    ) -> Self {
        Self {
            _chain_config: chain_config,
            p2p_config,
            messaging_handle,
            peer_event_receiver,
            peer_manager_sender,
            chainstate_handle,
            is_initial_block_download: true,
            peers: Default::default(),
        }
    }

    /// Runs the sync manager event loop.
    pub async fn run(&mut self) -> Result<Void> {
        log::info!("Starting SyncManager");

        let mut new_tip_receiver = self.subscribe_to_new_tip().await?;
        self.is_initial_block_download =
            self.chainstate_handle.call(|c| c.is_initial_block_download()).await??;

        loop {
            tokio::select! {
                event = self.peer_event_receiver.recv() => match event.ok_or(P2pError::ChannelClosed)? {
                    SyncControlEvent::Connected(peer_id) => self.register_peer(peer_id)?,
                    SyncControlEvent::Disconnected(peer_id) => self.unregister_peer(peer_id),
                },

                block_id = new_tip_receiver.recv() => {
                    // This error can only occur when chainstate drops an events subscriber.
                    let block_id = block_id.ok_or(P2pError::ChannelClosed)?;
                    self.handle_new_tip(block_id).await?;
                },

                event = self.messaging_handle.poll_next() => {
                    self.handle_peer_event(event?)?;
                },
            }
        }
    }

    /// Returns a receiver for the chainstate `NewTip` events.
    async fn subscribe_to_new_tip(&mut self) -> Result<UnboundedReceiver<Id<Block>>> {
        let (sender, receiver) = mpsc::unbounded_channel();

        let subscribe_func =
            Arc::new(
                move |chainstate_event: chainstate::ChainstateEvent| match chainstate_event {
                    chainstate::ChainstateEvent::NewTip(block_id, _) => {
                        let _ = sender.send(block_id).log_err_pfx("The new tip receiver closed");
                    }
                },
            );

        self.chainstate_handle
            .call_mut(|this| this.subscribe_to_events(subscribe_func))
            .await
            .map_err(|_| P2pError::SubsystemFailure)?;

        Ok(receiver)
    }

    // TODO: This shouldn't be public.
    // TODO: FIXME: Update the description.
    /// Registers the connected peer by creating a context for it.
    ///
    /// The `HeaderListRequest` message is sent to newly connected peers.
    pub fn register_peer(&mut self, peer: PeerId) -> Result<()> {
        log::debug!("Register peer {peer} to sync manager");

        let (sender, receiver) = mpsc::unbounded_channel();
        self.peers
            .insert(peer, sender)
            // This should never happen because a peer can only connect once.
            .map(|_| Err::<(), _>(P2pError::PeerError(PeerError::PeerAlreadyExists)))
            .transpose()?;

        tokio::spawn(async move {
            let mut peer = Peer::<T>::new(
                peer,
                Arc::clone(&self.p2p_config),
                self.chainstate_handle,
                self.messaging_handle,
                self.peer_manager_sender,
                receiver,
            );
            if let Err(e) = peer.run().await {
                log::error!("Sync manager peer ({}) error: {e:?}", peer.id());
            }
        });

        Ok(())
    }

    /// Stops the tasks of the given peer by closing the corresponding channel.
    fn unregister_peer(&mut self, peer: PeerId) {
        log::debug!("Unregister peer {peer} from sync manager");

        if self.peers.remove(&peer).is_some() {
            log::warn!("Unregistering unknown peer: {peer}");
        }
    }

    /// Announces the header of a new block to peers.
    async fn handle_new_tip(&mut self, block_id: Id<Block>) -> Result<()> {
        if self.is_initial_block_download {
            self.is_initial_block_download =
                self.chainstate_handle.call(|c| c.is_initial_block_download()).await??;
        }

        if self.is_initial_block_download {
            return Ok(());
        }

        let header = self
            .chainstate_handle
            .call(move |c| c.get_block(block_id))
            .await??
            // This should never happen because this block has just been produced by chainstate.
            .expect("A new tip block unavailable")
            .header()
            .clone();
        self.messaging_handle.make_announcement(Announcement::Block(header))
    }

    /// Sends an event to the corresponding peer.
    fn handle_peer_event(&mut self, event: SyncingEvent) -> Result<()> {
        let peer = match event {
            SyncingEvent::Message { peer, message: _ } => peer,
            SyncingEvent::Announcement {
                peer,
                announcement: _,
            } => peer,
        };

        let peer_channel = match self.peers.get(&peer) {
            Some(c) => c,
            None => {
                log::warn!("Received a message from unknown peer ({peer}): {event:?}");
                return Ok(());
            }
        };

        peer_channel.send(event).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests;
