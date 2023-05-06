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

use clap::Parser;
use wallet_cli_lib::{
    config::WalletCliArgs,
    console::{ConsoleOutput, StdioConsole},
};

#[tokio::main]
async fn main() {
    let args = WalletCliArgs::parse();
    let mut console = StdioConsole;
    wallet_cli_lib::run(console.clone(), args).await.unwrap_or_else(|err| {
        console.print_error(err);
        std::process::exit(1);
    })
}