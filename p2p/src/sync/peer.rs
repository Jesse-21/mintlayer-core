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

use std::{
    collections::{BTreeSet, VecDeque},
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use itertools::Itertools;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use void::Void;

use chainstate::chainstate_interface::ChainstateInterface;
use chainstate::{ban_score::BanScore, BlockError, BlockSource, ChainstateError, Locator};
use common::{
    chain::{block::BlockHeader, Block, SignedTransaction},
    primitives::{BlockHeight, Id, Idable},
};
use logging::log;
use mempool::{
    error::{Error as MempoolError, TxValidationError},
    MempoolHandle,
};
use utils::const_value::ConstValue;

use crate::{
    config::P2pConfig,
    error::{P2pError, PeerError, ProtocolError},
    message::{
        Announcement, BlockListRequest, BlockResponse, HeaderListRequest, HeaderListResponse,
        SyncMessage,
    },
    net::NetworkingService,
    types::peer_id::PeerId,
    utils::oneshot_nofail,
    MessagingService, PeerManagerEvent, Result,
};

#[derive(Debug)]
pub enum PeerEvent {
    Message { message: SyncMessage },
    Announcement { announcement: Box<Announcement> },
}

// TODO: Investigate if we need some kind of "timeouts" (waiting for blocks or headers).
// TODO: Track the block availability for a peer.
// TODO: Track the best known block for a peer and take into account the chain work when syncing.
/// A peer context.
///
/// Syncing logic runs in a separate task for each peer.
pub struct Peer<T: NetworkingService> {
    id: ConstValue<PeerId>,
    p2p_config: Arc<P2pConfig>,
    chainstate_handle: subsystem::Handle<Box<dyn ChainstateInterface>>,
    mempool_handle: MempoolHandle,
    peer_manager_sender: UnboundedSender<PeerManagerEvent<T>>,
    messaging_handle: T::MessagingHandle,
    events_receiver: UnboundedReceiver<PeerEvent>,
    is_initial_block_download: Arc<AtomicBool>,
    /// A list of headers received via the `HeaderListResponse` message that we haven't yet
    /// requested the blocks for.
    known_headers: Vec<BlockHeader>,
    /// A list of blocks that we requested from this peer.
    requested_blocks: BTreeSet<Id<Block>>,
    /// A queue of the blocks requested this peer.
    blocks_queue: VecDeque<Id<Block>>,
    /// The height of the best known block of a peer.
    best_known_block: Option<BlockHeight>,
}

impl<T> Peer<T>
where
    T: NetworkingService,
    T::MessagingHandle: MessagingService,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PeerId,
        p2p_config: Arc<P2pConfig>,
        chainstate_handle: subsystem::Handle<Box<dyn ChainstateInterface>>,
        mempool_handle: MempoolHandle,
        peer_manager_sender: UnboundedSender<PeerManagerEvent<T>>,
        messaging_handle: T::MessagingHandle,
        events_receiver: UnboundedReceiver<PeerEvent>,
        is_initial_block_download: Arc<AtomicBool>,
    ) -> Self {
        Self {
            id: id.into(),
            p2p_config,
            chainstate_handle,
            mempool_handle,
            peer_manager_sender,
            messaging_handle,
            events_receiver,
            is_initial_block_download,
            known_headers: Vec::new(),
            requested_blocks: BTreeSet::new(),
            blocks_queue: VecDeque::new(),
            best_known_block: None,
        }
    }

    /// Returns an identifier of the peer associated with this task.
    pub fn id(&self) -> PeerId {
        *self.id
    }

    pub async fn run(&mut self) -> Result<Void> {
        // TODO: Improve the initial header exchange. See the
        // https://github.com/mintlayer/mintlayer-core/issues/747 issue for details.
        self.request_headers().await?;

        loop {
            tokio::select! {
                event = self.events_receiver.recv() => {
                    let event = event.ok_or(P2pError::ChannelClosed)?;
                    self.handle_event(event).await?;
                },

                block_to_send_to_peer = async { self.blocks_queue.pop_front().expect("The block queue is empty") }, if !self.blocks_queue.is_empty() => {
                    self.send_block(block_to_send_to_peer).await?;
                }
            }
        }
    }

    async fn request_headers(&mut self) -> Result<()> {
        // TODO: Improve the initial header exchange. See the
        // https://github.com/mintlayer/mintlayer-core/issues/747 issue for details.
        let locator = self.chainstate_handle.call(|this| this.get_locator()).await??;
        debug_assert!(locator.len() <= *self.p2p_config.msg_max_locator_count);

        self.messaging_handle.send_message(
            self.id(),
            SyncMessage::HeaderListRequest(HeaderListRequest::new(locator)),
        )
    }

    async fn handle_event(&mut self, event: PeerEvent) -> Result<()> {
        let res = match event {
            PeerEvent::Message { message } => self.handle_message(message).await,
            PeerEvent::Announcement { announcement } => {
                self.handle_announcement(*announcement).await
            }
        };
        self.handle_result(res).await
    }

    async fn handle_message(&mut self, message: SyncMessage) -> Result<()> {
        match message {
            SyncMessage::HeaderListRequest(r) => self.handle_header_request(r.into_locator()).await,
            SyncMessage::BlockListRequest(r) => self.handle_block_request(r.into_block_ids()).await,
            SyncMessage::HeaderListResponse(r) => {
                self.handle_header_response(r.into_headers()).await
            }
            SyncMessage::BlockResponse(r) => self.handle_block_response(r.into_block()).await,
        }
    }

    /// Processes a header request by sending requested data to the peer.
    async fn handle_header_request(&mut self, locator: Locator) -> Result<()> {
        log::debug!("Headers request from peer {}", self.id());

        if locator.len() > *self.p2p_config.msg_max_locator_count {
            return Err(P2pError::ProtocolError(ProtocolError::LocatorSizeExceeded(
                locator.len(),
                *self.p2p_config.msg_max_locator_count,
            )));
        }
        log::trace!("locator: {locator:#?}");

        if self.is_initial_block_download.load(Ordering::Acquire) {
            // TODO: Check if a peer has permissions to ask for headers during the initial block download.
            log::debug!("Ignoring headers request because the node is in initial block download");
            return Ok(());
        }

        let limit = *self.p2p_config.msg_header_count_limit;
        let headers = self.chainstate_handle.call(move |c| c.get_headers(locator, limit)).await??;
        debug_assert!(headers.len() <= limit);
        self.messaging_handle.send_message(
            self.id(),
            SyncMessage::HeaderListResponse(HeaderListResponse::new(headers)),
        )
    }

    /// Processes the blocks request.
    async fn handle_block_request(&mut self, block_ids: Vec<Id<Block>>) -> Result<()> {
        utils::ensure!(
            !block_ids.is_empty(),
            P2pError::ProtocolError(ProtocolError::ZeroBlocksInRequest)
        );

        log::debug!(
            "Blocks request from peer {}: {}-{} ({})",
            self.id(),
            block_ids.first().expect("block_ids is not empty"),
            block_ids.last().expect("block_ids is not empty"),
            block_ids.len(),
        );

        if self.is_initial_block_download.load(Ordering::Acquire) {
            log::debug!("Ignoring blocks request because the node is in initial block download");
            return Ok(());
        }

        // Check that a peer doesn't exceed the blocks limit.
        self.p2p_config
            .max_request_blocks_count
            .checked_sub(block_ids.len())
            .and_then(|n| n.checked_sub(self.blocks_queue.len()))
            .ok_or(P2pError::ProtocolError(
                ProtocolError::BlocksRequestLimitExceeded(
                    block_ids.len() + self.blocks_queue.len(),
                    *self.p2p_config.max_request_blocks_count,
                ),
            ))?;
        log::trace!("Requested block ids: {block_ids:#?}");

        // Check that all the blocks are known and haven't been already requested.
        let ids = block_ids.clone();
        let best_known_block = self.best_known_block.unwrap_or(0.into());
        self.chainstate_handle
            .call(move |c| {
                for id in ids {
                    let index = c.get_block_index(&id)?.ok_or(P2pError::ProtocolError(
                        ProtocolError::UnknownBlockRequested(id),
                    ))?;

                    if index.block_height() <= best_known_block {
                        return Err(P2pError::ProtocolError(
                            ProtocolError::DuplicatedBlockRequest(id),
                        ));
                    }
                }
                Result::<_>::Ok(())
            })
            .await??;

        self.blocks_queue.extend(block_ids.into_iter());

        Ok(())
    }

    async fn handle_header_response(&mut self, headers: Vec<BlockHeader>) -> Result<()> {
        log::debug!("Headers response from peer {}", self.id());

        if !self.known_headers.is_empty() {
            return Err(P2pError::ProtocolError(ProtocolError::UnexpectedMessage(
                "headers response",
            )));
        }

        if headers.len() > *self.p2p_config.msg_header_count_limit {
            return Err(P2pError::ProtocolError(
                ProtocolError::HeadersLimitExceeded(
                    headers.len(),
                    *self.p2p_config.msg_header_count_limit,
                ),
            ));
        }
        log::trace!("Received headers: {headers:#?}");

        // TODO: Should the empty headers response be treated as misbehavior if we are going to
        // send a locator starting with the block preceding the tip?
        if headers.is_empty() {
            return Ok(());
        }

        // Each header must be connected to the previous one.
        if !headers
            .iter()
            .tuple_windows()
            .all(|(left, right)| &left.get_id() == right.prev_block_id())
        {
            return Err(P2pError::ProtocolError(ProtocolError::DisconnectedHeaders));
        }

        // The first header must be connected to a known block.
        let prev_id = *headers
            .first()
            // This is OK because of the `headers.is_empty()` check above.
            .expect("Headers shouldn't be empty")
            .prev_block_id();
        if self
            .chainstate_handle
            .call(move |c| c.get_gen_block_index(&prev_id))
            .await??
            .is_none()
        {
            return Err(P2pError::ProtocolError(ProtocolError::DisconnectedHeaders));
        }

        let is_max_headers = headers.len() == *self.p2p_config.msg_header_count_limit;
        let headers = self
            .chainstate_handle
            .call(|c| c.filter_already_existing_blocks(headers))
            .await??;
        if headers.is_empty() {
            // A peer can have more headers if we have received the maximum amount of them.
            if is_max_headers {
                self.request_headers().await?;
            }
            return Ok(());
        }

        // Only the first header can be checked with the `preliminary_header_check` function.
        let first_header = headers
            .first()
            // This is OK because of the `headers.is_empty()` check above.
            .expect("Headers shouldn't be empty")
            .clone();
        self.chainstate_handle
            .call(|c| c.preliminary_header_check(first_header))
            .await??;

        self.request_blocks(headers)
    }

    async fn handle_block_response(&mut self, block: Block) -> Result<()> {
        log::debug!("Block ({}) from peer {}", block.get_id(), self.id());

        if self.requested_blocks.take(&block.get_id()).is_none() {
            return Err(P2pError::ProtocolError(ProtocolError::UnexpectedMessage(
                "block response",
            )));
        }

        let block = self.chainstate_handle.call(|c| c.preliminary_block_check(block)).await??;
        match self
            .chainstate_handle
            .call_mut(|c| c.process_block(block, BlockSource::Peer))
            .await?
        {
            Ok(_) => Ok(()),
            // It is OK to receive an already processed block.
            Err(ChainstateError::ProcessBlockError(BlockError::BlockAlreadyExists(_))) => Ok(()),
            Err(e) => Err(e),
        }?;

        if self.requested_blocks.is_empty() {
            if self.known_headers.is_empty() {
                // Request more headers.
                self.request_headers().await?;
            } else {
                // Download remaining blocks.
                let mut headers = Vec::new();
                mem::swap(&mut headers, &mut self.known_headers);
                self.request_blocks(headers)?;
            }
        }

        Ok(())
    }

    async fn handle_announcement(&mut self, announcement: Announcement) -> Result<()> {
        match announcement {
            Announcement::Block(header) => self.handle_block_announcement(*header).await,
            Announcement::Transaction(tx) => self.handle_transaction_announcement(tx).await,
        }
    }

    async fn handle_block_announcement(&mut self, header: BlockHeader) -> Result<()> {
        let block_id = header.block_id();
        log::debug!(
            "Block announcement from peer {}: {block_id}: {header:?}",
            self.id()
        );

        if !self.requested_blocks.is_empty() {
            // We will download this block as part of syncing anyway.
            return Ok(());
        }

        // Do not request the block if it is already known
        if self
            .chainstate_handle
            .call(move |c| c.get_block_index(&block_id))
            .await??
            .is_some()
        {
            return Ok(());
        }

        let prev_id = *header.prev_block_id();
        if self
            .chainstate_handle
            .call(move |c| c.get_gen_block_index(&prev_id))
            .await??
            .is_none()
        {
            // TODO: Investigate this case. This can be used by malicious peers for a DoS attack.
            self.request_headers().await?;
            return Ok(());
        }

        let header_ = header.clone();
        self.chainstate_handle.call(|c| c.preliminary_header_check(header_)).await??;
        self.request_blocks(vec![header])
    }

    async fn handle_transaction_announcement(&mut self, tx: SignedTransaction) -> Result<()> {
        self.mempool_handle
            .call_async_mut(|m| m.add_transaction(tx))
            .await?
            .map_err(Into::into)
    }

    /// Handles a result of message processing.
    ///
    /// There are three possible types of errors:
    /// - Fatal errors will be propagated by this function effectively stopping the peer event loop.
    /// - Non-fatal errors aren't propagated, but the peer score will be increased by the
    ///   "ban score" value of the given error.
    /// - Ignored errors aren't propagated and don't affect the peer score.
    pub async fn handle_result(&mut self, result: Result<()>) -> Result<()> {
        let error = match result {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };

        match error {
            // Due to the fact that p2p is split into several tasks, it is possible to send a
            // request/response after a peer is disconnected, but before receiving the disconnect
            // event. Therefore this error can be safely ignored.
            P2pError::PeerError(PeerError::PeerDoesntExist) => Ok(()),
            P2pError::MempoolError(
                MempoolError::MempoolFull
                // TODO: https://github.com/mintlayer/mintlayer-core/issues/770
                | MempoolError::TxValidationError(TxValidationError::TransactionAlreadyInMempool),
            ) => Ok(()),
            // A protocol error - increase the ban score of a peer.
            e @ (P2pError::ProtocolError(_)
            | P2pError::MempoolError(MempoolError::TxValidationError(_))
            | P2pError::ChainstateError(ChainstateError::ProcessBlockError(
                BlockError::CheckBlockFailed(_),
            ))) => {
                log::info!(
                    "Adjusting the '{}' peer score by {}: {e:?}",
                    self.id(),
                    e.ban_score(),
                );

                let (sender, receiver) = oneshot_nofail::channel();
                self.peer_manager_sender.send(PeerManagerEvent::AdjustPeerScore(
                    self.id(),
                    e.ban_score(),
                    sender,
                ))?;
                receiver.await?.or_else(|e| match e {
                    P2pError::PeerError(PeerError::PeerDoesntExist) => Ok(()),
                    e => Err(e),
                })
            }
            // Some of these errors aren't technically fatal, but they shouldn't occur in the sync
            // manager.
            e @ (P2pError::DialError(_)
            | P2pError::ConversionError(_)
            | P2pError::PeerError(_)
            | P2pError::NoiseHandshakeError(_)
            | P2pError::InvalidConfigurationValue(_)
            | P2pError::ChainstateError(_)) => Err(e),
            // Fatal errors, simply propagate them to stop the sync manager.
            e @ (P2pError::ChannelClosed
            | P2pError::SubsystemFailure
            | P2pError::StorageFailure(_)
            | P2pError::InvalidStorageState(_)
            | P2pError::MempoolError(_)) => Err(e),
        }
    }

    /// Sends a block list request.
    ///
    /// The number of headers sent equals to `P2pConfig::requested_blocks_limit`, the remaining
    /// headers are stored in the peer context.
    fn request_blocks(&mut self, mut headers: Vec<BlockHeader>) -> Result<()> {
        debug_assert!(self.known_headers.is_empty());

        // Remove already requested blocks.
        headers.retain(|h| !self.requested_blocks.contains(&h.get_id()));
        if headers.is_empty() {
            return Ok(());
        }

        if headers.len() > *self.p2p_config.max_request_blocks_count {
            self.known_headers = headers.split_off(*self.p2p_config.max_request_blocks_count);
        }

        let block_ids: Vec<_> = headers.into_iter().map(|h| h.get_id()).collect();
        log::debug!(
            "Request blocks from peer {}: {}-{} ({})",
            self.id(),
            block_ids.first().expect("block_ids is not empty"),
            block_ids.last().expect("block_ids is not empty"),
            block_ids.len(),
        );
        self.messaging_handle.send_message(
            self.id(),
            SyncMessage::BlockListRequest(BlockListRequest::new(block_ids.clone())),
        )?;
        self.requested_blocks.extend(block_ids);

        Ok(())
    }

    async fn send_block(&mut self, id: Id<Block>) -> Result<()> {
        let (block, height) = self
            .chainstate_handle
            .call(move |c| {
                let height = c.get_block_height_in_main_chain(&id.into());
                let block = c.get_block(id);
                (block, height)
            })
            .await?;
        // All requested blocks are already checked while processing `BlockListRequest`.
        let block = block?.unwrap_or_else(|| panic!("Unknown block requested: {id}"));
        let height = height?;
        self.best_known_block = height;

        self.messaging_handle.send_message(
            self.id(),
            SyncMessage::BlockResponse(BlockResponse::new(block)),
        )
    }
}
