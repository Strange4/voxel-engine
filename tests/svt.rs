use glam::UVec3;
use voxel_engine::svt::{Svt, SvtNode};

// its kinda like a sphere
#[rustfmt::skip]
const SAMPLE_VOXEL_DATA: [u8; 64] = [
    0, 0, 0, 0,
    0, 1, 1, 0,
    0, 1, 1, 0,
    0, 0, 0, 0,

    0, 1, 1, 0,
    1, 1, 1, 1,
    1, 1, 1, 1,
    0, 1, 1, 0,

    0, 1, 1, 0,
    1, 1, 1, 1,
    1, 1, 1, 1,
    0, 1, 1, 0,

    0, 0, 0, 0,
    0, 1, 1, 0,
    0, 1, 1, 0,
    0, 0, 0, 0,
];

const SAMPLE_SIZE: UVec3 = UVec3::new(4, 4, 4);

#[test]
fn test_build_tree_level1() {
    let tree = Svt::build_tree(&SAMPLE_SIZE, &SAMPLE_VOXEL_DATA);

    // There's only 32 voxels that have some data
    assert_eq!(tree.leaf_data.len(), 32);
    assert_eq!(tree.leaf_data.as_slice(), &[1; 32]);

    // There should only be one node (the root) since it fits in a single 4^3 box.
    assert_eq!(tree.node_pool.len(), 1);

    let root_node: SvtNode = (&tree.node_pool[0]).into();

    assert_eq!(root_node.is_leaf, true);

    // Since there is only one node in this tree and it is a leaf,
    // the leaf data should point to the start of the leaf data vector
    assert_eq!(root_node.child_pointer, 0);

    const WANTED_MASK: u64 =
        0b0000_0110_0110_0000_0110_1111_1111_0110_0110_1111_1111_0110_0000_0110_0110_0000;
    assert_eq!(root_node.child_mask, WANTED_MASK);
}
