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

use super::TokenId;
use crate::{
    chain::{Transaction, TxOutput},
    primitives::id::hash_encoded,
};

pub fn token_id(tx: &Transaction) -> Option<TokenId> {
    Some(TokenId::new(hash_encoded(tx.inputs().get(0)?)))
}

pub fn get_tokens_issuance_count(outputs: &[TxOutput]) -> usize {
    outputs.iter().filter(|&output| output.is_token_or_nft_issuance()).count()
}
