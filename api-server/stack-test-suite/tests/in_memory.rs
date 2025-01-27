// Copyright (c) 2023 RBB S.r.l
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

mod v1;

use api_server_common::storage::impls::in_memory::transactional::TransactionalApiServerInMemoryStorage;
use api_web_server::{api::web_server, ApiServerWebServerState};
use common::chain::config::create_unit_test_config;
use std::{net::TcpListener, sync::Arc};

pub async fn spawn_webserver(url: &str) -> (tokio::task::JoinHandle<()>, reqwest::Response) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let task = tokio::spawn(async move {
        let web_server_state = {
            let chain_config = Arc::new(create_unit_test_config());
            let storage = TransactionalApiServerInMemoryStorage::new(&chain_config);

            ApiServerWebServerState {
                db: Arc::new(storage),
                chain_config: Arc::clone(&chain_config),
            }
        };

        web_server(listener, web_server_state).await.unwrap();
    });

    // Given that the listener port is open, this will block until a
    // response is made (by the web server, which takes the listener
    // over)
    let response = reqwest::get(format!("http://{}:{}{url}", addr.ip(), addr.port()))
        .await
        .unwrap();

    (task, response)
}

#[tokio::test]
async fn server_status() {
    let (task, response) = spawn_webserver("/").await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), r#"{"versions":["1.0.0"]}"#);

    task.abort();
}

#[tokio::test]
async fn bad_request() {
    let (task, response) = spawn_webserver("/non-existent-url").await;

    assert_eq!(response.status(), 400);
    assert_eq!(response.text().await.unwrap(), r#"{"error":"Bad request"}"#);

    task.abort();
}
