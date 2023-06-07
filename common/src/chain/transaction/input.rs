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

use crate::primitives::Amount;
use serialization::{Decode, Encode};

use super::{AccountOutPoint, AccountType, OutPointSourceId, UtxoOutPoint};

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Encode, Decode)]
pub enum TxInput {
    Utxo(UtxoOutPoint),
    /// `Amount` type represents the amount to withdraw from account.
    /// It helps solving 2 problems: calculating fees and providing ability to sign input balance with the witness.
    Account(AccountOutPoint, Amount),
}

impl TxInput {
    pub fn from_utxo(outpoint_source_id: OutPointSourceId, output_index: u32) -> Self {
        TxInput::Utxo(UtxoOutPoint::new(outpoint_source_id, output_index))
    }

    pub fn from_account(nonce: u128, account: AccountType, withdraw_amount: Amount) -> Self {
        TxInput::Account(AccountOutPoint::new(nonce, account), withdraw_amount)
    }

    pub fn utxo_outpoint(&self) -> Option<&UtxoOutPoint> {
        match self {
            TxInput::Utxo(outpoint) => Some(outpoint),
            TxInput::Account(_, _) => None,
        }
    }
}

impl From<UtxoOutPoint> for TxInput {
    fn from(outpoint: UtxoOutPoint) -> TxInput {
        TxInput::Utxo(outpoint)
    }
}
