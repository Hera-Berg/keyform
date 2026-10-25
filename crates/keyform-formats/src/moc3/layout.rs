//! moc3 section table. spec: 10-formats/moc3.md §4, §6

/// Indices into the count-info block (spec §4, 0-based).
#[allow(dead_code)]
pub mod c {
    pub const PARTS: usize = 0;
    pub const DEFORMERS: usize = 1;
    pub const WARPS: usize = 2;
    pub const ROTATIONS: usize = 3;
    pub const ART_MESHES: usize = 4;
    pub const PARAMETERS: usize = 5;
    pub const PART_KEYFORMS: usize = 6;
    pub const WARP_KEYFORMS: usize = 7;
    pub const ROTATION_KEYFORMS: usize = 8;
    pub const ART_MESH_KEYFORMS: usize = 9;
    pub const KEYFORM_POSITIONS: usize = 10;
    pub const PARAM_BINDING_INDICES: usize = 11;
    pub const KEYFORM_BINDINGS: usize = 12;
    pub const PARAM_BINDINGS: usize = 13;
    pub const KEYS: usize = 14;
    pub const UVS: usize = 15;
    pub const VERTEX_INDICES: usize = 16;
    pub const CLIP_MASKS: usize = 17;
    pub const DRAW_ORDER_GROUPS: usize = 18;
    pub const DRAW_ORDER_GROUP_ITEMS: usize = 19;
    pub const GLUES: usize = 20;
    pub const GLUE_PAIRS: usize = 21;
    pub const GLUE_KEYFORMS: usize = 22;
    pub const MULTIPLY_COLORS: usize = 23;
    pub const SCREEN_COLORS: usize = 24;
    pub const BS_PARAM_BINDINGS: usize = 25;
    pub const BS_KEYFORM_BINDINGS: usize = 26;
    pub const BS_WARPS: usize = 27;
    pub const BS_ART_MESHES: usize = 28;
    pub const BS_CONSTRAINT_INDICES: usize = 29;
    pub const BS_CONSTRAINTS: usize = 30;
    pub const BS_CONSTRAINT_VALUES: usize = 31;
    pub const BS_PARTS: usize = 32;
    pub const BS_ROTATIONS: usize = 33;
    pub const BS_GLUES: usize = 34;
    pub const OFFSCREENS: usize = 35;
    pub const OFFSCREEN_KEYFORMS: usize = 36;
    pub const BS_OFFSCREENS: usize = 37;

    /// Number of counts present in a file of `version`.
    pub fn present(version: u8) -> usize {
        match version {
            0..=3 => 23,
            4 => 32,
            5 => 35,
            _ => 38,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Zero-filled runtime scratch: 8 bytes per object.
    Runtime,
    /// 64-byte zero-padded ID strings.
    Ids,
    I32,
    F32,
    I16,
    U8,
}

impl Kind {
    pub fn elem_size(self) -> usize {
        match self {
            Kind::Runtime => 8,
            Kind::Ids => 64,
            Kind::I32 | Kind::F32 => 4,
            Kind::I16 => 2,
            Kind::U8 => 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Section {
    /// Row number in spec §6 (1-based; 1 and 2 are count / canvas info).
    pub row: usize,
    /// First header version byte that has this section.
    pub since: u8,
    pub kind: Kind,
    /// Count-info index giving the element count.
    pub count: usize,
    pub name: &'static str,
}

macro_rules! s {
    ($row:expr, $since:expr, $kind:ident, $count:ident, $name:expr) => {
        Section {
            row: $row,
            since: $since,
            kind: Kind::$kind,
            count: c::$count,
            name: $name,
        }
    };
}

/// Data sections in file order (spec §6 rows 3…167).
pub const SECTIONS: &[Section] = &[
    s!(3, 1, Runtime, PARTS, "parts.runtime"),
    s!(4, 1, Ids, PARTS, "parts.id"),
    s!(5, 1, I32, PARTS, "parts.keyform_binding"),
    s!(6, 1, I32, PARTS, "parts.keyform_begin"),
    s!(7, 1, I32, PARTS, "parts.keyform_count"),
    s!(8, 1, I32, PARTS, "parts.visible"),
    s!(9, 1, I32, PARTS, "parts.enabled"),
    s!(10, 1, I32, PARTS, "parts.parent_part"),
    s!(11, 1, Runtime, DEFORMERS, "deformers.runtime"),
    s!(12, 1, Ids, DEFORMERS, "deformers.id"),
    s!(13, 1, I32, DEFORMERS, "deformers.keyform_binding"),
    s!(14, 1, I32, DEFORMERS, "deformers.visible"),
    s!(15, 1, I32, DEFORMERS, "deformers.enabled"),
    s!(16, 1, I32, DEFORMERS, "deformers.parent_part"),
    s!(17, 1, I32, DEFORMERS, "deformers.parent_deformer"),
    s!(18, 1, I32, DEFORMERS, "deformers.type"),
    s!(19, 1, I32, DEFORMERS, "deformers.specific_index"),
    s!(20, 1, I32, WARPS, "warps.keyform_binding"),
    s!(21, 1, I32, WARPS, "warps.keyform_begin"),
    s!(22, 1, I32, WARPS, "warps.keyform_count"),
    s!(23, 1, I32, WARPS, "warps.vertex_count"),
    s!(24, 1, I32, WARPS, "warps.rows"),
    s!(25, 1, I32, WARPS, "warps.cols"),
    s!(26, 1, I32, ROTATIONS, "rotations.keyform_binding"),
    s!(27, 1, I32, ROTATIONS, "rotations.keyform_begin"),
    s!(28, 1, I32, ROTATIONS, "rotations.keyform_count"),
    s!(29, 1, F32, ROTATIONS, "rotations.base_angle"),
    s!(30, 1, Runtime, ART_MESHES, "art_meshes.runtime0"),
    s!(31, 1, Runtime, ART_MESHES, "art_meshes.runtime1"),
    s!(32, 1, Runtime, ART_MESHES, "art_meshes.runtime2"),
    s!(33, 1, Runtime, ART_MESHES, "art_meshes.runtime3"),
    s!(34, 1, Ids, ART_MESHES, "art_meshes.id"),
    s!(35, 1, I32, ART_MESHES, "art_meshes.keyform_binding"),
    s!(36, 1, I32, ART_MESHES, "art_meshes.keyform_begin"),
    s!(37, 1, I32, ART_MESHES, "art_meshes.keyform_count"),
    s!(38, 1, I32, ART_MESHES, "art_meshes.visible"),
    s!(39, 1, I32, ART_MESHES, "art_meshes.enabled"),
    s!(40, 1, I32, ART_MESHES, "art_meshes.parent_part"),
    s!(41, 1, I32, ART_MESHES, "art_meshes.parent_deformer"),
    s!(42, 1, I32, ART_MESHES, "art_meshes.texture"),
    s!(43, 1, U8, ART_MESHES, "art_meshes.flags"),
    s!(44, 1, I32, ART_MESHES, "art_meshes.vertex_count"),
    s!(45, 1, I32, ART_MESHES, "art_meshes.uv_begin"),
    s!(46, 1, I32, ART_MESHES, "art_meshes.index_begin"),
    s!(47, 1, I32, ART_MESHES, "art_meshes.index_count"),
    s!(48, 1, I32, ART_MESHES, "art_meshes.clip_begin"),
    s!(49, 1, I32, ART_MESHES, "art_meshes.clip_count"),
    s!(50, 1, Runtime, PARAMETERS, "parameters.runtime"),
    s!(51, 1, Ids, PARAMETERS, "parameters.id"),
    s!(52, 1, F32, PARAMETERS, "parameters.max"),
    s!(53, 1, F32, PARAMETERS, "parameters.min"),
    s!(54, 1, F32, PARAMETERS, "parameters.default"),
    s!(55, 1, I32, PARAMETERS, "parameters.repeat"),
    s!(56, 1, I32, PARAMETERS, "parameters.decimal_places"),
    s!(57, 1, I32, PARAMETERS, "parameters.binding_begin"),
    s!(58, 1, I32, PARAMETERS, "parameters.binding_count"),
    s!(59, 1, F32, PART_KEYFORMS, "part_keyforms.draw_order"),
    s!(60, 1, F32, WARP_KEYFORMS, "warp_keyforms.opacity"),
    s!(61, 1, I32, WARP_KEYFORMS, "warp_keyforms.position_begin"),
    s!(62, 1, F32, ROTATION_KEYFORMS, "rotation_keyforms.opacity"),
    s!(63, 1, F32, ROTATION_KEYFORMS, "rotation_keyforms.angle"),
    s!(64, 1, F32, ROTATION_KEYFORMS, "rotation_keyforms.origin_x"),
    s!(65, 1, F32, ROTATION_KEYFORMS, "rotation_keyforms.origin_y"),
    s!(66, 1, F32, ROTATION_KEYFORMS, "rotation_keyforms.scale"),
    s!(67, 1, I32, ROTATION_KEYFORMS, "rotation_keyforms.reflect_x"),
    s!(68, 1, I32, ROTATION_KEYFORMS, "rotation_keyforms.reflect_y"),
    s!(69, 1, F32, ART_MESH_KEYFORMS, "art_mesh_keyforms.opacity"),
    s!(
        70,
        1,
        F32,
        ART_MESH_KEYFORMS,
        "art_mesh_keyforms.draw_order"
    ),
    s!(
        71,
        1,
        I32,
        ART_MESH_KEYFORMS,
        "art_mesh_keyforms.position_begin"
    ),
    s!(72, 1, F32, KEYFORM_POSITIONS, "keyform_positions.xy"),
    s!(73, 1, I32, PARAM_BINDING_INDICES, "param_binding_indices"),
    s!(74, 1, I32, KEYFORM_BINDINGS, "keyform_bindings.begin"),
    s!(75, 1, I32, KEYFORM_BINDINGS, "keyform_bindings.count"),
    s!(76, 1, I32, PARAM_BINDINGS, "param_bindings.keys_begin"),
    s!(77, 1, I32, PARAM_BINDINGS, "param_bindings.keys_count"),
    s!(78, 1, F32, KEYS, "keys"),
    s!(79, 1, F32, UVS, "uvs"),
    s!(80, 1, I16, VERTEX_INDICES, "vertex_indices"),
    s!(81, 1, I32, CLIP_MASKS, "clip_masks"),
    s!(
        82,
        1,
        I32,
        DRAW_ORDER_GROUPS,
        "draw_order_groups.item_begin"
    ),
    s!(
        83,
        1,
        I32,
        DRAW_ORDER_GROUPS,
        "draw_order_groups.item_count"
    ),
    s!(
        84,
        1,
        I32,
        DRAW_ORDER_GROUPS,
        "draw_order_groups.total_drawn"
    ),
    s!(
        85,
        1,
        I32,
        DRAW_ORDER_GROUPS,
        "draw_order_groups.max_draw_order"
    ),
    s!(
        86,
        1,
        I32,
        DRAW_ORDER_GROUPS,
        "draw_order_groups.min_draw_order"
    ),
    s!(
        87,
        1,
        I32,
        DRAW_ORDER_GROUP_ITEMS,
        "draw_order_group_items.type"
    ),
    s!(
        88,
        1,
        I32,
        DRAW_ORDER_GROUP_ITEMS,
        "draw_order_group_items.index"
    ),
    s!(
        89,
        1,
        I32,
        DRAW_ORDER_GROUP_ITEMS,
        "draw_order_group_items.own_group"
    ),
    s!(90, 1, Runtime, GLUES, "glues.runtime"),
    s!(91, 1, Ids, GLUES, "glues.id"),
    s!(92, 1, I32, GLUES, "glues.keyform_binding"),
    s!(93, 1, I32, GLUES, "glues.keyform_begin"),
    s!(94, 1, I32, GLUES, "glues.keyform_count"),
    s!(95, 1, I32, GLUES, "glues.mesh_a"),
    s!(96, 1, I32, GLUES, "glues.mesh_b"),
    s!(97, 1, I32, GLUES, "glues.pair_begin"),
    s!(98, 1, I32, GLUES, "glues.pair_count"),
    s!(99, 1, F32, GLUE_PAIRS, "glue_pairs.weight"),
    s!(100, 1, I16, GLUE_PAIRS, "glue_pairs.vertex"),
    s!(101, 1, F32, GLUE_KEYFORMS, "glue_keyforms.intensity"),
    s!(102, 2, I32, WARPS, "warps.quad"),
    s!(103, 4, Runtime, PARAMETERS, "param_key_lists.runtime"),
    s!(104, 4, I32, PARAMETERS, "param_key_lists.keys_begin"),
    s!(105, 4, I32, PARAMETERS, "param_key_lists.keys_count"),
    s!(106, 4, I32, WARPS, "warps.color_begin"),
    s!(107, 4, I32, ROTATIONS, "rotations.color_begin"),
    s!(108, 4, I32, ART_MESHES, "art_meshes.color_begin"),
    s!(109, 4, F32, MULTIPLY_COLORS, "multiply_colors.r"),
    s!(110, 4, F32, MULTIPLY_COLORS, "multiply_colors.g"),
    s!(111, 4, F32, MULTIPLY_COLORS, "multiply_colors.b"),
    s!(112, 4, F32, SCREEN_COLORS, "screen_colors.r"),
    s!(113, 4, F32, SCREEN_COLORS, "screen_colors.g"),
    s!(114, 4, F32, SCREEN_COLORS, "screen_colors.b"),
    s!(115, 4, I32, PARAMETERS, "parameters.type"),
    s!(116, 4, I32, PARAMETERS, "parameters.bs_binding_begin"),
    s!(117, 4, I32, PARAMETERS, "parameters.bs_binding_count"),
    s!(
        118,
        4,
        I32,
        BS_PARAM_BINDINGS,
        "bs_param_bindings.keys_begin"
    ),
    s!(
        119,
        4,
        I32,
        BS_PARAM_BINDINGS,
        "bs_param_bindings.keys_count"
    ),
    s!(120, 4, I32, BS_PARAM_BINDINGS, "bs_param_bindings.base_key"),
    s!(
        121,
        4,
        I32,
        BS_KEYFORM_BINDINGS,
        "bs_keyform_bindings.param_binding"
    ),
    s!(
        122,
        4,
        I32,
        BS_KEYFORM_BINDINGS,
        "bs_keyform_bindings.keyform_begin"
    ),
    s!(
        123,
        4,
        I32,
        BS_KEYFORM_BINDINGS,
        "bs_keyform_bindings.keyform_count"
    ),
    s!(
        124,
        4,
        I32,
        BS_KEYFORM_BINDINGS,
        "bs_keyform_bindings.constraint_begin"
    ),
    s!(
        125,
        4,
        I32,
        BS_KEYFORM_BINDINGS,
        "bs_keyform_bindings.constraint_count"
    ),
    s!(126, 4, I32, BS_WARPS, "bs_warps.target"),
    s!(127, 4, I32, BS_WARPS, "bs_warps.binding_begin"),
    s!(128, 4, I32, BS_WARPS, "bs_warps.binding_count"),
    s!(129, 4, I32, BS_ART_MESHES, "bs_art_meshes.target"),
    s!(130, 4, I32, BS_ART_MESHES, "bs_art_meshes.binding_begin"),
    s!(131, 4, I32, BS_ART_MESHES, "bs_art_meshes.binding_count"),
    s!(132, 4, I32, BS_CONSTRAINT_INDICES, "bs_constraint_indices"),
    s!(133, 4, I32, BS_CONSTRAINTS, "bs_constraints.parameter"),
    s!(134, 4, I32, BS_CONSTRAINTS, "bs_constraints.value_begin"),
    s!(135, 4, I32, BS_CONSTRAINTS, "bs_constraints.value_count"),
    s!(
        136,
        4,
        F32,
        BS_CONSTRAINT_VALUES,
        "bs_constraint_values.key"
    ),
    s!(
        137,
        4,
        F32,
        BS_CONSTRAINT_VALUES,
        "bs_constraint_values.weight"
    ),
    s!(138, 5, I32, WARP_KEYFORMS, "warp_keyforms.multiply_color"),
    s!(139, 5, I32, WARP_KEYFORMS, "warp_keyforms.screen_color"),
    s!(
        140,
        5,
        I32,
        ROTATION_KEYFORMS,
        "rotation_keyforms.multiply_color"
    ),
    s!(
        141,
        5,
        I32,
        ROTATION_KEYFORMS,
        "rotation_keyforms.screen_color"
    ),
    s!(
        142,
        5,
        I32,
        ART_MESH_KEYFORMS,
        "art_mesh_keyforms.multiply_color"
    ),
    s!(
        143,
        5,
        I32,
        ART_MESH_KEYFORMS,
        "art_mesh_keyforms.screen_color"
    ),
    s!(144, 5, I32, BS_PARTS, "bs_parts.target"),
    s!(145, 5, I32, BS_PARTS, "bs_parts.binding_begin"),
    s!(146, 5, I32, BS_PARTS, "bs_parts.binding_count"),
    s!(147, 5, I32, BS_ROTATIONS, "bs_rotations.target"),
    s!(148, 5, I32, BS_ROTATIONS, "bs_rotations.binding_begin"),
    s!(149, 5, I32, BS_ROTATIONS, "bs_rotations.binding_count"),
    s!(150, 5, I32, BS_GLUES, "bs_glues.target"),
    s!(151, 5, I32, BS_GLUES, "bs_glues.binding_begin"),
    s!(152, 5, I32, BS_GLUES, "bs_glues.binding_count"),
    s!(153, 6, I32, PARTS, "parts.offscreen"),
    s!(154, 6, I32, ART_MESHES, "art_meshes.blend_mode"),
    s!(155, 6, Runtime, OFFSCREENS, "offscreens.runtime"),
    s!(156, 6, I32, OFFSCREENS, "offscreens.owner"),
    s!(157, 6, U8, OFFSCREENS, "offscreens.flags"),
    s!(158, 6, I32, OFFSCREENS, "offscreens.blend_mode"),
    s!(159, 6, I32, OFFSCREENS, "offscreens.clip_begin"),
    s!(160, 6, I32, OFFSCREENS, "offscreens.clip_count"),
    s!(
        161,
        6,
        I32,
        PART_KEYFORMS,
        "part_keyforms.offscreen_keyform"
    ),
    s!(
        162,
        6,
        F32,
        OFFSCREEN_KEYFORMS,
        "offscreen_keyforms.opacity"
    ),
    s!(
        163,
        6,
        I32,
        OFFSCREEN_KEYFORMS,
        "offscreen_keyforms.multiply_color"
    ),
    s!(
        164,
        6,
        I32,
        OFFSCREEN_KEYFORMS,
        "offscreen_keyforms.screen_color"
    ),
    s!(165, 6, I32, BS_OFFSCREENS, "bs_offscreens.target"),
    s!(166, 6, I32, BS_OFFSCREENS, "bs_offscreens.binding_begin"),
    s!(167, 6, I32, BS_OFFSCREENS, "bs_offscreens.binding_count"),
];

pub const HEADER_LEN: usize = 64;

/// Section-offset table entries for `version`.
pub fn table_entries(version: u8) -> usize {
    if version >= 6 {
        480
    } else {
        160
    }
}

/// Offset of the count-info block.
pub fn data_base(version: u8) -> usize {
    if version >= 6 {
        5824
    } else {
        1984
    }
}

pub fn count_info_len(version: u8) -> usize {
    if version >= 5 {
        256
    } else {
        128
    }
}

pub const CANVAS_INFO_LEN: usize = 64;

pub fn align64(n: usize) -> usize {
    n.div_ceil(64) * 64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_ordered_and_complete() {
        for (i, s) in SECTIONS.iter().enumerate() {
            assert_eq!(s.row, i + 3, "{}", s.name);
            assert!(
                s.count < c::present(s.since),
                "{} count index beyond its version",
                s.name
            );
        }
        assert_eq!(SECTIONS.len(), 165);
        // Data must start after the table.
        for v in 1..=6 {
            assert!(HEADER_LEN + 4 * table_entries(v) <= data_base(v));
        }
    }
}
