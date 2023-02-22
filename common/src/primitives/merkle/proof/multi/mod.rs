// Copyright (c) 2021-2023 RBB S.r.l
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

use std::collections::BTreeMap;

use itertools::Itertools;

use crate::primitives::merkle::{
    tree::{MerkleTree, Node},
    MerkleTreeProofExtractionError,
};

use super::single::SingleProofNodes;

// Merkle proofs for multiple leaves.
pub struct MultiProofNodes<'a> {
    /// The leaves where the calculation will start
    leaves: Vec<Node<'a>>,
    /// The minimal set of nodes needed to recreate the root hash
    nodes: Vec<Node<'a>>,
}

/// Ensure the leaves indices are sorted and unique
fn is_sorted_and_unique(leaves_indices: &[usize]) -> bool {
    leaves_indices.iter().tuple_windows::<(&usize, &usize)>().all(|(i, j)| i < j)
}

impl<'a> MultiProofNodes<'a> {
    pub fn from_tree_leaves(
        tree: &'a MerkleTree,
        leaves_indices: &[usize],
    ) -> Result<Self, MerkleTreeProofExtractionError> {
        if !is_sorted_and_unique(leaves_indices) {
            return Err(
                MerkleTreeProofExtractionError::UnsortedOrUniqueLeavesIndices(
                    leaves_indices.to_vec(),
                ),
            );
        }

        let leaves_count = tree.leaves_count();

        if leaves_indices.iter().any(|v| *v >= leaves_count.get()) {
            return Err(MerkleTreeProofExtractionError::IndexOutOfRange(
                leaves_indices.to_vec(),
                leaves_count.get(),
            ));
        }

        let single_proofs = leaves_indices
            .iter()
            .map(|i| SingleProofNodes::from_tree_leaf(tree, *i))
            .collect::<Result<Vec<_>, _>>()?;

        let mut computed_from_prev_level = vec![];

        let mut proof = vec![];

        let mut level = 0;
        while level < leaves_count.get() {
            let leaves = single_proofs
                .iter()
                .map(|sp| (sp.branch()[level].abs_index(), sp.branch()[level]))
                .collect::<BTreeMap<usize, Node<'a>>>();

            let siblings = single_proofs
                .iter()
                .map(|sp| sp.branch()[level].sibling().unwrap())
                .collect::<Vec<_>>();

            // We remove leaves that are already in siblings because they will come from the verification input.
            // This happens when the leaves, for which a proof is requested, are used together to build a parent node
            // in the tree. In that case, given that the verification will have both as inputs, we don't need to include
            // them in the proof.
            // We also remove the nodes that can be computed from the previous level, because they will be included in the proof
            let proofs_at_level = siblings
                .into_iter()
                .filter(|node| !leaves.contains_key(&node.abs_index()))
                .filter(|node| !computed_from_prev_level.contains(&node.abs_index()))
                .collect::<Vec<_>>();

            // We collect all the nodes that can be computed from this level, and will use it in the next iteration
            computed_from_prev_level = proofs_at_level
                .iter()
                .tuple_windows::<(&Node, &Node)>()
                .filter(|n| n.0.abs_index() % 2 == 0 && n.0.abs_index() + 1 == n.1.abs_index())
                .map(|(n1, _n2)| n1.parent().unwrap().abs_index())
                .collect();

            proof.extend(proofs_at_level.iter().cloned());

            level += 1;
        }

        Ok(Self {
            leaves: leaves_indices
                .iter()
                .map(|i| tree.node_from_bottom(0, *i).expect("Leaves already checked"))
                .collect(),
            nodes: proof,
        })
    }

    pub fn nodes(&self) -> &[Node<'a>] {
        &self.nodes
    }

    pub fn leaves(&self) -> &[Node<'a>] {
        &self.leaves
    }
}

#[cfg(test)]
mod tests;
