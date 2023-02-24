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

use std::{
    fmt::{Display, Formatter},
    num::NonZeroUsize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TreeSize(usize);

const MAX_TREE_SIZE: usize = 1 << 31;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TreeSizeError {
    #[error("Zero is invalid size for tree")]
    ZeroSize,
    #[error("Tree size must be power of two minus one; this value was found: {0}")]
    InvalidSize(usize),
    #[error("Tree with this huge size is not supported: {0}")]
    HugeTreeUnsupported(usize),
}

impl TreeSize {
    pub fn get(&self) -> usize {
        self.0
    }

    pub fn leaf_count(&self) -> NonZeroUsize {
        ((self.0 + 1) / 2).try_into().expect("Guaranteed by construction")
    }

    pub fn level_count(&self) -> NonZeroUsize {
        (self.0.count_ones() as usize).try_into().expect("Guaranteed by construction")
    }

    pub fn from_value(value: usize) -> Result<Self, TreeSizeError> {
        Self::try_from(value)
    }

    pub fn from_leaf_count(leaf_count: usize) -> Result<Self, TreeSizeError> {
        if leaf_count == 0 {
            return Err(TreeSizeError::ZeroSize);
        }
        Self::try_from(leaf_count * 2 - 1)
    }
}

impl TryFrom<usize> for TreeSize {
    type Error = TreeSizeError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value == 0 {
            Err(TreeSizeError::ZeroSize)
        } else if (value + 1).count_ones() != 1 {
            Err(TreeSizeError::InvalidSize(value))
        } else if value > MAX_TREE_SIZE {
            Err(TreeSizeError::HugeTreeUnsupported(value))
        } else {
            Ok(Self(value))
        }
    }
}

impl From<TreeSize> for usize {
    fn from(tree_size: TreeSize) -> Self {
        tree_size.0
    }
}

impl AsRef<usize> for TreeSize {
    fn as_ref(&self) -> &usize {
        &self.0
    }
}

impl Display for TreeSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_from_tree_size() {
        assert_eq!(TreeSize::try_from(0), Err(TreeSizeError::ZeroSize));
        assert_eq!(TreeSize::try_from(1), Ok(TreeSize(1)));
        assert_eq!(TreeSize::try_from(2), Err(TreeSizeError::InvalidSize(2)));
        assert_eq!(TreeSize::try_from(3), Ok(TreeSize(3)));
        assert_eq!(TreeSize::try_from(4), Err(TreeSizeError::InvalidSize(4)));
        assert_eq!(TreeSize::try_from(5), Err(TreeSizeError::InvalidSize(5)));
        assert_eq!(TreeSize::try_from(6), Err(TreeSizeError::InvalidSize(6)));
        assert_eq!(TreeSize::try_from(7), Ok(TreeSize(7)));
        assert_eq!(TreeSize::try_from(8), Err(TreeSizeError::InvalidSize(8)));
        assert_eq!(TreeSize::try_from(9), Err(TreeSizeError::InvalidSize(9)));
        assert_eq!(TreeSize::try_from(10), Err(TreeSizeError::InvalidSize(10)));
        assert_eq!(TreeSize::try_from(11), Err(TreeSizeError::InvalidSize(11)));
        assert_eq!(TreeSize::try_from(12), Err(TreeSizeError::InvalidSize(12)));
        assert_eq!(TreeSize::try_from(13), Err(TreeSizeError::InvalidSize(13)));
        assert_eq!(TreeSize::try_from(14), Err(TreeSizeError::InvalidSize(14)));
        assert_eq!(TreeSize::try_from(15), Ok(TreeSize(15)));
        assert_eq!(TreeSize::try_from(16), Err(TreeSizeError::InvalidSize(16)));

        for i in 1..1000usize {
            if (i + 1).count_ones() == 1 {
                assert_eq!(TreeSize::try_from(i), Ok(TreeSize(i)));
                assert_eq!(TreeSize::from_value(i), Ok(TreeSize(i)));
            } else {
                assert_eq!(TreeSize::try_from(i), Err(TreeSizeError::InvalidSize(i)));
                assert_eq!(TreeSize::from_value(i), Err(TreeSizeError::InvalidSize(i)));
            }
        }
    }

    #[test]
    fn construction_from_leaves_count() {
        assert_eq!(TreeSize::from_leaf_count(0), Err(TreeSizeError::ZeroSize));
        assert_eq!(TreeSize::from_leaf_count(1), Ok(TreeSize(1)));
        assert_eq!(TreeSize::from_leaf_count(2), Ok(TreeSize(3)));
        assert_eq!(
            TreeSize::from_leaf_count(3),
            Err(TreeSizeError::InvalidSize(5))
        );
        assert_eq!(TreeSize::from_leaf_count(4), Ok(TreeSize(7)));
        assert_eq!(
            TreeSize::from_leaf_count(5),
            Err(TreeSizeError::InvalidSize(9))
        );
        assert_eq!(
            TreeSize::from_leaf_count(6),
            Err(TreeSizeError::InvalidSize(11))
        );
        assert_eq!(
            TreeSize::from_leaf_count(7),
            Err(TreeSizeError::InvalidSize(13))
        );
        assert_eq!(TreeSize::from_leaf_count(8), Ok(TreeSize(15)));
        assert_eq!(
            TreeSize::from_leaf_count(9),
            Err(TreeSizeError::InvalidSize(17))
        );
        assert_eq!(
            TreeSize::from_leaf_count(10),
            Err(TreeSizeError::InvalidSize(19))
        );
        assert_eq!(
            TreeSize::from_leaf_count(11),
            Err(TreeSizeError::InvalidSize(21))
        );
        assert_eq!(
            TreeSize::from_leaf_count(12),
            Err(TreeSizeError::InvalidSize(23))
        );
        assert_eq!(
            TreeSize::from_leaf_count(13),
            Err(TreeSizeError::InvalidSize(25))
        );
        assert_eq!(
            TreeSize::from_leaf_count(14),
            Err(TreeSizeError::InvalidSize(27))
        );
        assert_eq!(
            TreeSize::from_leaf_count(15),
            Err(TreeSizeError::InvalidSize(29))
        );
        assert_eq!(TreeSize::from_leaf_count(16), Ok(TreeSize(31)));
        assert_eq!(
            TreeSize::from_leaf_count(17),
            Err(TreeSizeError::InvalidSize(33))
        );
    }

    #[test]
    fn calculations() {
        let t1 = TreeSize::try_from(1).unwrap();
        assert_eq!(t1.get(), 1);
        assert_eq!(t1.leaf_count().get(), 1);
        assert_eq!(t1.level_count().get(), 1);

        let t3 = TreeSize::try_from(3).unwrap();
        assert_eq!(t3.get(), 3);
        assert_eq!(t3.leaf_count().get(), 2);
        assert_eq!(t3.level_count().get(), 2);

        let t7 = TreeSize::try_from(7).unwrap();
        assert_eq!(t7.get(), 7);
        assert_eq!(t7.leaf_count().get(), 4);
        assert_eq!(t7.level_count().get(), 3);

        let t15 = TreeSize::try_from(15).unwrap();
        assert_eq!(t15.get(), 15);
        assert_eq!(t15.leaf_count().get(), 8);
        assert_eq!(t15.level_count().get(), 4);

        let t31 = TreeSize::try_from(31).unwrap();
        assert_eq!(t31.get(), 31);
        assert_eq!(t31.leaf_count().get(), 16);
        assert_eq!(t31.level_count().get(), 5);

        let t63 = TreeSize::try_from(63).unwrap();
        assert_eq!(t63.get(), 63);
        assert_eq!(t63.leaf_count().get(), 32);
        assert_eq!(t63.level_count().get(), 6);

        let t127 = TreeSize::try_from(127).unwrap();
        assert_eq!(t127.get(), 127);
        assert_eq!(t127.leaf_count().get(), 64);
        assert_eq!(t127.level_count().get(), 7);

        let t255 = TreeSize::try_from(255).unwrap();
        assert_eq!(t255.get(), 255);
        assert_eq!(t255.leaf_count().get(), 128);
        assert_eq!(t255.level_count().get(), 8);
    }
}
