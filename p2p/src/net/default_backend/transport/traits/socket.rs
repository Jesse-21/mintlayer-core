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

use async_trait::async_trait;
use futures::future::BoxFuture;
use p2p_types::socket_address::SocketAddress;

use crate::Result;

use super::{listener::TransportListener, stream::PeerStream};

/// An abstraction layer for the transport layer at the highest level, which is responsible for:
/// 1. Binding to a socket at a specific port, where we listen to connections.
///    The mechanism to retrieve new connected clients are up to the listener struct
/// 2. Providing the connect function, that's used to connect to other peers
#[async_trait]
pub trait TransportSocket: Send + Sync + 'static {
    /// A listener type (or acceptor as per boost terminology).
    type Listener: TransportListener<Stream = Self::Stream>;

    /// A messages stream.
    type Stream: PeerStream;

    /// Creates a new listener bound to the specified address.
    async fn bind(&self, address: Vec<SocketAddress>) -> Result<Self::Listener>;

    /// Returns a future that opens a connection to the given address.
    fn connect(&self, address: SocketAddress) -> BoxFuture<'static, Result<Self::Stream>>;
}
