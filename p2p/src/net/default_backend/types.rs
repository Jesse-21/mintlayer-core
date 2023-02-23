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

use std::collections::BTreeSet;

use common::primitives::semver::SemVer;
use serialization::{Decode, Encode};

use crate::{
    error,
    message::{
        AddrListRequest, AddrListResponse, AnnounceAddrRequest, AnnounceAddrResponse, Announcement,
        BlockListRequest, BlockResponse, HeaderListRequest, HeaderListResponse, PeerManagerMessage,
        PingRequest, PingResponse, SyncMessage,
    },
    net::{
        default_backend::transport::TransportSocket,
        types::{PeerInfo, PubSubTopic},
    },
    types::{peer_address::PeerAddress, peer_id::PeerId},
};

#[derive(Debug)]
pub enum Command<T: TransportSocket> {
    Connect {
        address: T::Address,
    },
    Disconnect {
        peer_id: PeerId,
    },
    SendMessage {
        peer: PeerId,
        message: Message,
    },
    AnnounceData {
        topic: PubSubTopic,
        message: Vec<u8>,
    },
}

pub enum SyncingEvent {
    Message {
        peer: PeerId,
        message: Box<SyncMessage>,
    },
    Announcement {
        peer: PeerId,
        announcement: Box<Announcement>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectivityEvent<T: TransportSocket> {
    Message {
        peer: PeerId,
        message: PeerManagerMessage,
    },
    InboundAccepted {
        address: T::Address,
        peer_info: PeerInfo,
        receiver_address: Option<PeerAddress>,
    },
    OutboundAccepted {
        address: T::Address,
        peer_info: PeerInfo,
        receiver_address: Option<PeerAddress>,
    },
    ConnectionError {
        address: T::Address,
        error: error::P2pError,
    },
    ConnectionClosed {
        peer_id: PeerId,
    },
    /// A peer misbehaved and its reputation must be adjusted according to the error type.
    Misbehaved {
        peer_id: PeerId,
        error: error::P2pError,
    },
}

/// Random nonce sent in outbound handshake.
/// Used to detect and drop self connections.
pub type HandshakeNonce = u64;

#[derive(Debug, PartialEq, Eq)]
pub enum PeerEvent {
    /// Peer information received from remote
    PeerInfoReceived {
        network: [u8; 4],
        version: SemVer,
        subscriptions: BTreeSet<PubSubTopic>,
        receiver_address: Option<PeerAddress>,

        /// For outbound connections that is what we sent.
        /// For inbound connections that is what was received from remote peer.
        handshake_nonce: HandshakeNonce,
    },

    /// Connection closed to remote
    ConnectionClosed,

    /// Message received from remote
    MessageReceived { message: Message },
}

/// Events sent by the default_backend backend to peers
#[derive(Debug)]
pub enum Event {
    Disconnect,
    SendMessage(Box<Message>),
}

// TODO: Decide what to do about protocol upgrades.
// For example adding new address type to PeerAddress might break handshakes with older nodes.
#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub enum HandshakeMessage {
    Hello {
        version: SemVer,
        network: [u8; 4],
        subscriptions: BTreeSet<PubSubTopic>,

        /// Socket address of the remote peer as seen by this node (addr_you in bitcoin)
        receiver_address: Option<PeerAddress>,

        /// Random nonce that is only used to detect and drop self-connects
        handshake_nonce: HandshakeNonce,
    },
    HelloAck {
        version: SemVer,
        network: [u8; 4],
        subscriptions: BTreeSet<PubSubTopic>,

        /// Socket address of the remote peer as seen by this node (addr_you in bitcoin)
        receiver_address: Option<PeerAddress>,
    },
}

#[derive(Debug, Encode, Decode, PartialEq, Eq, Clone)]
pub enum Message {
    Handshake(HandshakeMessage),
    HeaderListRequest(HeaderListRequest),
    BlockListRequest(BlockListRequest),
    AddrListRequest(AddrListRequest),
    AnnounceAddrRequest(AnnounceAddrRequest),
    PingRequest(PingRequest),
    HeaderListResponse(HeaderListResponse),
    BlockResponse(BlockResponse),
    AddrListResponse(AddrListResponse),
    AnnounceAddrResponse(AnnounceAddrResponse),
    PingResponse(PingResponse),
    Announcement(Box<Announcement>),
}

impl From<PeerManagerMessage> for Message {
    fn from(message: PeerManagerMessage) -> Self {
        match message {
            PeerManagerMessage::AddrListRequest(r) => Message::AddrListRequest(r),
            PeerManagerMessage::AnnounceAddrRequest(r) => Message::AnnounceAddrRequest(r),
            PeerManagerMessage::PingRequest(r) => Message::PingRequest(r),
            PeerManagerMessage::AddrListResponse(r) => Message::AddrListResponse(r),
            PeerManagerMessage::AnnounceAddrResponse(r) => Message::AnnounceAddrResponse(r),
            PeerManagerMessage::PingResponse(r) => Message::PingResponse(r),
        }
    }
}

impl From<SyncMessage> for Message {
    fn from(message: SyncMessage) -> Self {
        match message {
            SyncMessage::HeaderListRequest(r) => Message::HeaderListRequest(r),
            SyncMessage::BlockListRequest(r) => Message::BlockListRequest(r),
            SyncMessage::HeaderListResponse(r) => Message::HeaderListResponse(r),
            SyncMessage::BlockResponse(r) => Message::BlockResponse(r),
        }
    }
}
