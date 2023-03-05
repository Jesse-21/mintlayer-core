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

use std::{fmt::Debug, sync::Arc};

use tokio::sync::mpsc;

use chainstate::{ban_score::BanScore, BlockError, ChainstateError, CheckBlockError};
use common::{
    chain::block::{timestamp::BlockTimestamp, Block, BlockReward, ConsensusData},
    primitives::Idable,
};

use p2p::{
    config::P2pConfig,
    error::P2pError,
    event::PeerManagerEvent,
    message::{Announcement, HeaderListResponse, SyncMessage},
    net::{types::SyncingEvent, ConnectivityService, NetworkingService, SyncingMessagingService},
    sync::BlockSyncManager,
    testing_utils::{connect_services, TestTransportMaker},
};

tests![invalid_pubsub_block,];

// Start two network services, spawn a `SyncMessageHandler` for the first service, publish an
// invalid block from the first service and verify that the `SyncManager` of the first service
// receives a `AdjustPeerScore` event which bans the peer of the second service.
async fn invalid_pubsub_block<T, N, A>()
where
    T: TestTransportMaker<Transport = N::Transport, Address = N::Address>,
    N: NetworkingService + Debug + 'static,
    N::ConnectivityHandle: ConnectivityService<N>,
    N::SyncingMessagingHandle: SyncingMessagingService<N>,
{
    let (_tx_sync, rx_sync) = mpsc::unbounded_channel();
    let (tx_peer_manager, mut rx_peer_manager) = mpsc::unbounded_channel();
    let chain_config = Arc::new(common::chain::config::create_unit_test_config());
    let p2p_config = Arc::new(P2pConfig::default());
    let handle = p2p_test_utils::start_chainstate(Arc::clone(&chain_config)).await;

    let (mut conn1, sync1) = N::start(
        T::make_transport(),
        vec![T::make_address()],
        Arc::clone(&chain_config),
        Default::default(),
    )
    .await
    .unwrap();

    let mut sync1 = BlockSyncManager::<N>::new(
        Arc::clone(&chain_config),
        Arc::clone(&p2p_config),
        sync1,
        handle.clone(),
        rx_sync,
        tx_peer_manager,
    );

    let (mut conn2, mut sync2) = N::start(
        T::make_transport(),
        vec![T::make_address()],
        Arc::clone(&chain_config),
        Arc::clone(&p2p_config),
    )
    .await
    .unwrap();

    let (_address, _peer_info1, peer_info2) = connect_services::<N>(&mut conn1, &mut conn2).await;

    // Create a block with an invalid timestamp.
    let block = Block::new(
        Vec::new(),
        chain_config.genesis_block().get_id().into(),
        BlockTimestamp::from_int_seconds(1),
        ConsensusData::None,
        BlockReward::new(Vec::new()),
    )
    .unwrap();

    tokio::spawn(async move {
        sync1.register_peer(peer_info2.peer_id).unwrap();
        sync1.run().await
    });

    // spawn `sync2` into background and spam an orphan block on the network
    tokio::spawn(async move {
        let peer = match sync2.poll_next().await.unwrap() {
            SyncingEvent::Message {
                peer,
                message: SyncMessage::HeaderListRequest(_),
            } => peer,
            e => panic!("Unexpected event type: {e:?}"),
        };
        sync2
            .send_message(
                peer,
                SyncMessage::HeaderListResponse(HeaderListResponse::new(Vec::new())),
            )
            .unwrap();

        sync2.make_announcement(Announcement::Block(block.header().clone())).unwrap();
    });

    match rx_peer_manager.recv().await {
        Some(PeerManagerEvent::AdjustPeerScore(peer_id, score, _)) => {
            assert_eq!(peer_id, peer_info2.peer_id);
            assert_eq!(
                score,
                P2pError::ChainstateError(ChainstateError::ProcessBlockError(
                    BlockError::CheckBlockFailed(CheckBlockError::BlockTimeOrderInvalid)
                ))
                .ban_score()
            );
        }
        e => panic!("invalid event received: {e:?}"),
    }
}
