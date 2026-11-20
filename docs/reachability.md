# Reachability

How a person reaches each command, as found by `tools/e2e/audit.py`
(it opens every menu, submenu, context menu, dock tab, combo box and `…`
dialog in several states and records what is visible, on top and
enabled). Generated; do not edit by hand. Notes for entries the
automatic walk cannot reach are in `tools/e2e/reachability-notes.tsv`,
unfixed gaps in `tools/e2e/reachability-known-gaps.tsv`.

Checked: **yes** = seen by the audit; **by hand** = described in the
notes (a state the audit doesn't set up); **GAP** = no entry point yet.

## Commands

| Feature | How to reach it | Checked | Code |
|---|---|---|---|
| (none) | File ▸ Recent Files ▸ (none) (no document); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/form_dialogs.rs:565` |
| 4 Division | Mesh Edit ▸ 4 Division (mesh edit) | yes | `crates/keyform-app/src/app.rs:1473` |
| < Add all layers as new ArtMesh > | File ▸ Open a PSD with a model open: the PSD dialog lists these first (session step 1 uses the PSD flow without a model) | by hand | `crates/keyform-app/src/psd_dialog.rs:173` |
| < Register PSD file without adding ArtMeshes > | As above (PSD dialog with a model open) | by hand | `crates/keyform-app/src/psd_dialog.rs:177` |
| Add | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1124` |
| Add control point | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:779` |
| Add group | Model Settings (runtime model) ▸ Pose section | by hand | `crates/keyform-app/src/viewer_settings.rs:478` |
| Add HitArea meshes | Model Settings (runtime model) ▸ Hit Areas section | by hand | `crates/keyform-app/src/viewer_settings.rs:519` |
| Add image track… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1618` |
| Add key at playhead | Animation workspace: select an attribute row in the timeline, the curve editor shows it (session step: scene keys) | by hand | `crates/keyform-app/src/animation.rs:2189` |
| add line automatically | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:800` |
| Add mask… | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Model Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ BackHair (model) | yes | `crates/keyform-app/src/palettes.rs:1197` |
| Add model track | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1585` |
| Add motions… | Model Settings tab (runtime model); Log tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:154` |
| Add parameter | Model Settings (runtime model) ▸ Expressions, editing an expression | by hand | `crates/keyform-app/src/viewer_settings.rs:397` |
| Add parameter… | Animation ▸ Motion-sync Settings…: mapping | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:677` |
| Add scene track | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1609` |
| Add Single Point Controller | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:584` |
| Add sound track (WAV)… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1590` |
| Add texture… | dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:173` |
| Add to the current animation file | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:653` |
| Add… | dialog via Modeling ▸ Motion-sync Settings… (model); dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:311` |
| Adjust to position and shape | As above | by hand | `crates/keyform-app/src/template_dialog.rs:130` |
| Adjust to template | As above: blend-mode handling | by hand | `crates/keyform-app/src/template_dialog.rs:163` |
| Adjustment Mode 1 | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1682` |
| Adjustment Mode 2 | combo box 'Adjustment Mode 1' (animation); Deformer ▸ Adjustment Mode 1 (animation) | yes | `crates/keyform-app/src/animation.rs:1684` |
| All deformers | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:832` |
| All drawables | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:827` |
| All keyforms (not only the current one) | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:725` |
| All model images | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Add texture… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:310` |
| All objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:822` |
| Allow 90° rotation | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:225` |
| Alpha Blend | As above | by hand | `crates/keyform-app/src/template_dialog.rs:171` |
| Always show warnings regarding masks | Show ▸ Always show warnings regarding masks (no document); Show ▸ Always show warnings regarding masks (model) | yes | `crates/keyform-app/src/app.rs:948` |
| Angle | the toolbar ▸ Soft (model); Create Rotation Deformer dialog via Modeling ▸ Deformer ▸ Create rotation deformer… ▸ Create (model) | yes | `crates/keyform-app/src/physics_editor.rs:1053` |
| Animation | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1035` |
| Animation… | File ▸ New ▸ Animation… (no document); File ▸ New ▸ Animation… (model) | yes | `crates/keyform-app/src/app.rs:736` |
| Apply | Animation ▸ Motion-sync Settings… | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:539` |
| Apply 3D Rotation Expression… | Modeling ▸ Apply 3D Rotation Expression… (model); Modeling ▸ Apply 3D Rotation Expression… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1371` |
| Apply Expression | Animation ▸ Apply Expression (no document); Animation ▸ Apply Expression (model) | yes | `crates/keyform-app/src/app.rs:1546` |
| Apply form edit | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1687` |
| Apply Model Template… | Modeling ▸ Model Template ▸ Apply Model Template… (model); Modeling ▸ Model Template ▸ Apply Model Template… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1339` |
| Apply Physics | Animation ▸ Apply Physics (no document); Animation ▸ Apply Physics (model) | yes | `crates/keyform-app/src/app.rs:1548` |
| Approved | File ▸ External Application Integration Settings…: one checkbox per connected application (none connect in the audit) | by hand | `crates/keyform-app/src/external_host.rs:320` |
| ArtMesh | Modeling ▸ ArtMesh (model); Modeling ▸ ArtMesh (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1097` |
| Audio Elements | Animation ▸ Motion-sync Settings…: tab | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:453` |
| Auto attach Edge (line) | Mesh Edit ▸ Auto attach Edge (line) (mesh edit) | yes | `crates/keyform-app/src/app.rs:1448` |
| Auto Generation of Face Motion… | Modeling ▸ Auto Generation of Face Motion… (model); Modeling ▸ Auto Generation of Face Motion… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1381` |
| Auto Generation of Sway Motion… | Modeling ▸ Auto Generation of Sway Motion… (model); Modeling ▸ Auto Generation of Sway Motion… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1361` |
| Auto Layout… | dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:213` |
| Auto-update maximum of output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1182` |
| Automatic | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:237` |
| Automatic Eye-Blinking | Animation ▸ Automatic Eye-Blinking (no document); Animation ▸ Automatic Eye-Blinking (model) | yes | `crates/keyform-app/src/app.rs:1547` |
| Automatic mapping | As above | by hand | `crates/keyform-app/src/template_dialog.rs:131` |
| Automatic Mesh generator… | Modeling ▸ ArtMesh ▸ Automatic Mesh generator… (model); Modeling ▸ ArtMesh ▸ Automatic Mesh generator… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1121` |
| Axis: horizontal (top ↔ bottom) | Mesh Edit ▸ Mirror Edit ▸ Axis: horizontal (top ↔ bottom) (mesh edit) | yes | `crates/keyform-app/src/app.rs:3071` |
| Axis: vertical (left ↔ right) | Mesh Edit ▸ Mirror Edit ▸ Axis: vertical (left ↔ right) (mesh edit) | yes | `crates/keyform-app/src/app.rs:3066` |
| Bake Animation from Motion-sync | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1648` |
| Bake Animation from Parameter Controller | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:605` |
| Bake Animation from Physics… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1642` |
| Bake Lip-sync | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1660` |
| Batch Reflect… | Modeling ▸ Parameter ▸ Batch Reflect… (model); Modeling ▸ Parameter ▸ Batch Reflect… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1241` |
| Bind | Mesh edit of two meshes (select two meshes, Mesh edit): partner binding buttons | by hand | `crates/keyform-app/src/app.rs:3162` |
| Blend Form… | Modeling ▸ Edit Form ▸ Blend Form… (model); Modeling ▸ Edit Form ▸ Blend Form… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1277` |
| Bottom left | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:816` |
| Bottom right | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:817` |
| Breath | Animation ▸ Breath (no document); Animation ▸ Breath (model) | yes | `crates/keyform-app/src/app.rs:1559` |
| Bulk Set | Inspector with several meshes selected: draw order | by hand | `crates/keyform-app/src/inspector.rs:236` |
| Cancel | Home ▸ New Model… (no document); dialog via File ▸ Export Image/Video ▸ Image… (model) | yes | `crates/keyform-app/src/animation.rs:1451` |
| Cancel (Esc) | Toolbar ▸ Temp path (Tool Details panel), as Confirm | by hand | `crates/keyform-app/src/path_tool.rs:1212` |
| Cancel auto-adjusted culling | dialog via Modeling ▸ Edit Form ▸ Flip… (model); Flip dialog via Modeling ▸ Edit Form ▸ Flip… ▸ Parameters to reflect (model) | yes | `crates/keyform-app/src/form_dialogs.rs:750` |
| Center | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/template_dialog.rs:119` |
| Center of Canvas | Mesh Edit ▸ Mirror Edit ▸ Center of Canvas (mesh edit) | yes | `crates/keyform-app/src/app.rs:3083` |
| Center of Selected Vertices | Mesh Edit ▸ Mirror Edit ▸ Center of Selected Vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:3089` |
| Centre | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model); Options for Paste Form Special dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… ▸ Centre (model) | yes | `crates/keyform-app/src/form_dialogs.rs:684` |
| Centre (min + max) / 2 | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:804` |
| Clear | Log tab (no document); Tool Details tab (no document) | yes | `crates/keyform-app/src/palettes.rs:1367` |
| Close | File ▸ Close (no document); File ▸ Close (model) | yes | `crates/keyform-app/src/app.rs:771` |
| Close all | File ▸ Close all (no document); File ▸ Close all (model) | yes | `crates/keyform-app/src/app.rs:782` |
| Color Blend | As above | by hand | `crates/keyform-app/src/template_dialog.rs:170` |
| Confirm | Animation ▸ Motion-sync Settings… | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:544` |
| Confirm (Enter) | Toolbar ▸ Temp path, then click on the canvas: the Tool Details panel shows Confirm / Cancel | by hand | `crates/keyform-app/src/path_tool.rs:1209` |
| Connect | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3150` |
| Continuous creation | dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model); dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model) ▸ Set as child of selected deformer | yes | `crates/keyform-app/src/create_deformer.rs:163` |
| Controllers | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1705` |
| Copy | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1171` |
| Copy Form | Modeling ▸ Edit Form ▸ Copy Form (model); Modeling ▸ Edit Form ▸ Copy Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1252` |
| Copy output lines | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Edit (model) | yes | `crates/keyform-app/src/physics_editor.rs:254` |
| Create | Home ▸ New Model… (no document); dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model) | yes | `crates/keyform-app/src/create_deformer.rs:164` |
| Create a new animation file | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:658` |
| Create new scene | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1566` |
| Create rotation deformer… | Modeling ▸ Deformer ▸ Create rotation deformer… (model); Modeling ▸ Deformer ▸ Create rotation deformer… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1089` |
| Create Warp Deformer… | Modeling ▸ Deformer ▸ Create Warp Deformer… (model); Modeling ▸ Deformer ▸ Create Warp Deformer… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1079` |
| Culling | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Model Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ BackHair (model) | yes | `crates/keyform-app/src/palettes.rs:1150` |
| Dark Theme | Window ▸ Dark Theme (no document); Window ▸ Dark Theme (model) | yes | `crates/keyform-app/src/app.rs:1050` |
| Decrease Output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1142` |
| Decrease priority | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1546` |
| Default value | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:803` |
| Deformer | Window ▸ Deformer (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1075` |
| Delete | Edit ▸ Delete (no document); Edit ▸ Delete (model) | yes | `crates/keyform-app/src/app.rs:917` |
| Delete control point | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:782` |
| Delete Controller | Animation workspace ▸ Controllers menu, with a parameter controller selected | by hand | `crates/keyform-app/src/param_ctrl_ui.rs:634` |
| Delete Form Edit | Animation workspace ▸ Form Edit menu, after a form edit exists for the selected mesh | by hand | `crates/keyform-app/src/animation.rs:1700` |
| Delete key | context menu of a Parameter row ▸ Delete key (model); context menu of a Parameter row ▸ Delete key (model, mesh selected) | yes | `crates/keyform-app/src/animation.rs:2202` |
| Delete path | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:785` |
| Delete scene | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1577` |
| Delete the existing keys and add 3 keys | Modeling ▸ Auto Generation of Sway Motion… (and 3D rotation / face generators) when the chosen parameter doesn't have exactly 3 keys | by hand | `crates/keyform-app/src/generators_ui.rs:112` |
| Delete Vertex | Mesh Edit ▸ Delete Vertex (mesh edit) | yes | `crates/keyform-app/src/app.rs:1491` |
| Deselect | Edit ▸ Deselect (no document); Edit ▸ Deselect (model) | yes | `crates/keyform-app/src/app.rs:938` |
| Disable drag & drop | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:152` |
| Do not show again | Dialog "Issues on Mask Settings" (shown on export when a mask setting needs attention) | by hand | `crates/keyform-app/src/app.rs:2117` |
| Don't change | As above | by hand | `crates/keyform-app/src/template_dialog.rs:164` |
| Don't show again | Dialog "Paste Form: pair copies with targets" (Modeling ▸ Edit Form ▸ Paste Form with several copies) | by hand | `crates/keyform-app/src/form_dialogs.rs:574` |
| Don't show again. | Dialog "About Number of Keys for Extended Interpolation" (choosing extended interpolation for a parameter with fewer than 3 keys) | by hand | `crates/keyform-app/src/form_dialogs.rs:347` |
| Done | Glue tool (Modeling ▸ Glue…): Tool Details panel | by hand | `crates/keyform-app/src/app.rs:3341` |
| Down | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Down (model) | yes | `crates/keyform-app/src/physics_editor.rs:1071` |
| Draw order group | Inspector with a part selected | by hand | `crates/keyform-app/src/inspector.rs:445` |
| Duplicate | Motion-sync Settings dialog via Modeling ▸ Motion-sync Settings… ▸ Edit… (model); dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:319` |
| Duplicate… | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:798` |
| Edge swap | Mesh Edit ▸ Edge swap (mesh edit) | yes | `crates/keyform-app/src/app.rs:1468` |
| Edit | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:889` |
| Edit Form | Modeling ▸ Edit Form (model); Modeling ▸ Edit Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1248` |
| Edit Texture Atlas… | Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model); Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1305` |
| Edit… | dialog via Modeling ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:314` |
| Enable | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:417` |
| Enable external application integration | dialog via Show ▸ External Application Integration Settings… (no document) | yes | `crates/keyform-app/src/external_host.rs:271` |
| Enable Pose Switching | Animation ▸ Enable Pose Switching (no document); Animation ▸ Enable Pose Switching (model) | yes | `crates/keyform-app/src/app.rs:1549` |
| Events | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1710` |
| Exit | File ▸ Exit (no document); File ▸ Exit (model) | yes | `crates/keyform-app/src/app.rs:883` |
| Export as moc3 file… | File ▸ Export For Runtime ▸ Export as moc3 file… (no document); File ▸ Export For Runtime ▸ Export as moc3 file… (model) | yes | `crates/keyform-app/src/app.rs:846` |
| Export For Runtime | File ▸ Export For Runtime (no document); File ▸ Export For Runtime (model) | yes | `crates/keyform-app/src/app.rs:842` |
| Export Image/Video | File ▸ Export Image/Video (no document); File ▸ Export Image/Video (model) | yes | `crates/keyform-app/src/app.rs:815` |
| Export model template… | Modeling ▸ Model Template ▸ Export model template… (model); Modeling ▸ Model Template ▸ Export model template… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1349` |
| Export Parameter Controller… | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:652` |
| Export Physics settings… | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Physics (model) | yes | `crates/keyform-app/src/physics_editor.rs:275` |
| Extended Interpolation… | Modeling ▸ Parameter ▸ Extended Interpolation… (model); Modeling ▸ Parameter ▸ Extended Interpolation… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1233` |
| File | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:727` |
| Finish | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:1755` |
| Finish Mesh editing | Mesh Edit ▸ Finish Mesh editing (mesh edit) | yes | `crates/keyform-app/src/app.rs:1423` |
| Fit to parameter range | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1151` |
| Fit to View | Show ▸ Fit to View (no document); Show ▸ Fit to View (model) | yes | `crates/keyform-app/src/app.rs:975` |
| Fix Aspect Ratio | Home ▸ New Model… (no document) | yes | `crates/keyform-app/src/new_model_dialog.rs:56` |
| Flip horizontally | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:407` |
| Flip horizontally about x = | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:679` |
| Flip rotation deformer position | dialog via Modeling ▸ Edit Form ▸ Flip… (model); Flip dialog via Modeling ▸ Edit Form ▸ Flip… ▸ Parameters to reflect (model) | yes | `crates/keyform-app/src/form_dialogs.rs:746` |
| Flip sway horizontally | Modeling ▸ Auto Generation of Sway Motion… | by hand | `crates/keyform-app/src/generators_ui.rs:134` |
| Flip vertically | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:408` |
| Flip vertically about y = | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:689` |
| Flip… | Modeling ▸ Edit Form ▸ Flip… (model); Modeling ▸ Edit Form ▸ Flip… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1293` |
| Fold line | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:788` |
| Form Edit | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1676` |
| Generate and Register Glue | Modeling ▸ Glue ▸ Generate and Register Glue (model); Modeling ▸ Glue ▸ Generate and Register Glue (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1149` |
| GIF | dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation); dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:364` |
| Glue | Modeling ▸ Glue (model); Modeling ▸ Glue (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1132` |
| Group editing… | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:419` |
| Help | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1057` |
| Idle Motion | Animation ▸ Idle Motion (no document); Animation ▸ Idle Motion (model) | yes | `crates/keyform-app/src/app.rs:1554` |
| Image sequence / GIF / Video… | File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (no document); File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (model) | yes | `crates/keyform-app/src/app.rs:823` |
| Image… | File ▸ Export Image/Video ▸ Image… (no document); File ▸ Export Image/Video ▸ Image… (model) | yes | `crates/keyform-app/src/app.rs:835` |
| Import motion3.json… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1624` |
| Import Parameter Controller… | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:663` |
| Import Physics Settings… | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Physics (model) | yes | `crates/keyform-app/src/physics_editor.rs:268` |
| Include in motion export | Animation workspace ▸ Scene ▸ Scene settings… | by hand | `crates/keyform-app/src/animation.rs:1407` |
| Increase Output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1133` |
| Increase priority | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1542` |
| Initialize Calculate FPS | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Physics (model) | yes | `crates/keyform-app/src/physics_editor.rs:283` |
| Input settings | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:174` |
| Insert key to every frame | Animation workspace ▸ Scene ▸ Bake Animation from Physics…: key options | by hand | `crates/keyform-app/src/animation.rs:1520` |
| Invert Mask | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Model Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ BackHair (model) | yes | `crates/keyform-app/src/palettes.rs:1222` |
| Keyed only | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1930` |
| Language | Show ▸ Language (no document); Show ▸ Language (model) | yes | `crates/keyform-app/src/app.rs:952` |
| Limit Settings for Blend Shape Weights… | Modeling ▸ Parameter ▸ Limit Settings for Blend Shape Weights… (model); Modeling ▸ Parameter ▸ Limit Settings for Blend Shape Weights… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1201` |
| Link animation file… | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:684` |
| Load Audio… | dialog via Modeling ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:836` |
| Loop | Model Settings tab (model); Log tab (model) | yes | `crates/keyform-app/src/animation.rs:1911` |
| Loop motion | Animation workspace ▸ Scene ▸ Scene settings…: export options section | by hand | `crates/keyform-app/src/animation.rs:1398` |
| Make Background Transparent | dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation); dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:416` |
| Mapping Settings | Animation ▸ Motion-sync Settings…: tab (needs a motion-sync set; the audit's model has none) | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:448` |
| Mask | As above | by hand | `crates/keyform-app/src/template_dialog.rs:168` |
| Merge Vertices | Mesh Edit ▸ Merge Vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:1480` |
| Mesh Edit | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:1031` |
| Mesh edit | Modeling ▸ ArtMesh ▸ Mesh edit (model); Modeling ▸ ArtMesh ▸ Mesh edit (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1109` |
| Mesh reconstruction | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3194` |
| Mirror | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3198` |
| Mirror Edit | Mesh Edit ▸ Mirror Edit (mesh edit); Mesh Edit ▸ Mirror Edit ▸ Mirror Edit (mesh edit) | yes | `crates/keyform-app/src/app.rs:1484` |
| Model | File ▸ New ▸ Model (no document); File ▸ New ▸ Model (model) | yes | `crates/keyform-app/src/app.rs:730` |
| Model Template | Modeling ▸ Model Template (model); Modeling ▸ Model Template (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1335` |
| Model Template list | As above | by hand | `crates/keyform-app/src/template_dialog.rs:92` |
| Modeling | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1033` |
| Motion Mirroring… | Modeling ▸ Parameter ▸ Motion Mirroring… (model); Modeling ▸ Parameter ▸ Motion Mirroring… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1222` |
| Motion-sync | Animation ▸ Motion-sync (no document); Animation ▸ Motion-sync (model) | yes | `crates/keyform-app/src/app.rs:1550` |
| Motion-sync Settings… | Modeling ▸ Motion-sync Settings… (model); Modeling ▸ Motion-sync Settings… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1328` |
| Move Here | Drag a row in the Parts palette and right-click the drop target (context menu of a drop target) | by hand | `crates/keyform-app/src/palettes.rs:559` |
| New | File ▸ New (no document); File ▸ New (model) | yes | `crates/keyform-app/src/app.rs:728` |
| New from current values | Model Settings tab (runtime model); Log tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:260` |
| New Model… | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:2563` |
| New Part | Modeling ▸ Part ▸ New Part (model); Modeling ▸ Part ▸ New Part (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1181` |
| New part | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:132` |
| No | Closing a modified document: save prompt | by hand | `crates/keyform-app/src/app.rs:503` |
| No controller tool | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:598` |
| No thinning | dialog via Show ▸ External Application Integration Settings… (no document); External Application Integration Settings dialog via Show ▸ External Application Integration Settings… ▸ No thinning (no document) | yes | `crates/keyform-app/src/physics_editor.rs:664` |
| None | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ None (model) | yes | `crates/keyform-app/src/texture_atlas.rs:315` |
| Offscreen drawing | Inspector with a part selected | by hand | `crates/keyform-app/src/inspector.rs:456` |
| OK | dialog via File ▸ Export Image/Video ▸ Image… (model); dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/animation.rs:1450` |
| Open… | File ▸ Open… (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:751` |
| Options for Paste Form Special… | Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model); Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1285` |
| Output All | dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation); dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:374` |
| Output Settings | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:175` |
| Output Work Area | dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation); dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:379` |
| Parameter | Window ▸ Parameter (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1197` |
| Parameter repeat | Animation ▸ Parameter repeat (no document); Animation ▸ Parameter repeat (model) | yes | `crates/keyform-app/src/app.rs:1560` |
| Part | Modeling ▸ Part (model); Modeling ▸ Part (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1179` |
| Part Settings… | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:148` |
| Paste | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1175` |
| Paste Form | Modeling ▸ Edit Form ▸ Paste Form (model); Modeling ▸ Edit Form ▸ Paste Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1265` |
| Paste output lines | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Edit (model) | yes | `crates/keyform-app/src/physics_editor.rs:261` |
| Physics | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:266` |
| Physics Settings… | Modeling ▸ Physics Settings… (model); Modeling ▸ Physics Settings… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1318` |
| Play | dialog via Modeling ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:864` |
| Position X | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Position X (model) | yes | `crates/keyform-app/src/physics_editor.rs:1048` |
| Preview | Modeling ▸ Edit Form ▸ Blend Form…: preview checkbox | by hand | `crates/keyform-app/src/form_dialogs.rs:610` |
| Prioritize duplicate names | As above: mapping options | by hand | `crates/keyform-app/src/template_dialog.rs:129` |
| Protect Edge exterior | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3190` |
| Recent Files | File ▸ Recent Files (no document); File ▸ Recent Files (model) | yes | `crates/keyform-app/src/app.rs:756` |
| Record | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ▶ (model) | yes | `crates/keyform-app/src/physics_editor.rs:650` |
| Record motion and generate animation data | dialog via Show ▸ External Application Integration Settings… (no document) | yes | `crates/keyform-app/src/external_host.rs:286` |
| Redo | Edit ▸ Redo (no document); Edit ▸ Redo (model) | yes | `crates/keyform-app/src/app.rs:905` |
| Reflect | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/physics_editor.rs:908` |
| Reflect Horizontally | Show ▸ Reflect Horizontally (no document); Show ▸ Reflect Horizontally (model) | yes | `crates/keyform-app/src/app.rs:1021` |
| Remove | File ▸ External Application Integration Settings…: per connected application; also Model Settings rows (motions, expressions) when present | by hand | `crates/keyform-app/src/external_host.rs:321` |
| Remove from model3.json | Model Settings (runtime model): next to each listed motion (session step: viewer motion adds one) | by hand | `crates/keyform-app/src/viewer_settings.rs:121` |
| Remove redundant keys | External Application Integration Settings dialog via Show ▸ External Application Integration Settings… ▸ No thinning (no document); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:669` |
| Rename | Modeling ▸ Physics Settings…: group buttons ▸ Rename… shows the name field and this button | by hand | `crates/keyform-app/src/physics_editor.rs:1562` |
| Rename… | Motion-sync Settings dialog via Modeling ▸ Motion-sync Settings… ▸ Edit… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/animation.rs:1570` |
| Repeat | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ▶ (model) | yes | `crates/keyform-app/src/physics_editor.rs:649` |
| Reset | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/palettes.rs:808` |
| Reset Axis Position | Mesh Edit ▸ Mirror Edit ▸ Reset Axis Position (mesh edit) | yes | `crates/keyform-app/src/app.rs:3082` |
| Reset Current Layout | Window ▸ Reset Current Layout (no document); Window ▸ Reset Current Layout (model) | yes | `crates/keyform-app/src/app.rs:1044` |
| Reset expression | Model Settings (runtime model) ▸ Expressions, with an expression selected | by hand | `crates/keyform-app/src/viewer_settings.rs:242` |
| Reset Magnification | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1159` |
| Reset Mesh | Mesh Edit ▸ Reset Mesh (mesh edit) | yes | `crates/keyform-app/src/app.rs:1458` |
| Reset Output Maximum | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1165` |
| Reset parameters | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/app.rs:1764` |
| Reset Rotation | Show ▸ Reset Rotation (no document); Show ▸ Reset Rotation (model) | yes | `crates/keyform-app/src/app.rs:1007` |
| Restriction for SDK (handles move vertically only) | Animation workspace ▸ Scene ▸ Scene settings… | by hand | `crates/keyform-app/src/animation.rs:1401` |
| Reverse Clipping ID Lookup | Parts palette: right-click a mesh row that is used as a clipping mask | by hand | `crates/keyform-app/src/palettes.rs:613` |
| Run Auto Layout | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:252` |
| Save as… | File ▸ Save as… (no document); File ▸ Save as… (model) | yes | `crates/keyform-app/src/app.rs:807` |
| Save model3.json | File ▸ Save model3.json (runtime model); Model Settings tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:48` |
| Scene | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1561` |
| scene | As above with Video: frame-rate "scene" button | by hand | `crates/keyform-app/src/export_media.rs:408` |
| Scene settings… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1562` |
| Select all | Animation workspace ▸ Scene ▸ Bake Animation from Physics…: parameter list buttons | by hand | `crates/keyform-app/src/animation.rs:1499` |
| Select All | Edit ▸ Select All (no document); Edit ▸ Select All (model) | yes | `crates/keyform-app/src/app.rs:927` |
| Select Model Template… | Modeling ▸ Model Template ▸ Apply Model Template…, after choosing a template file | by hand | `crates/keyform-app/src/template_dialog.rs:86` |
| Select/Edit Controller | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:583` |
| Selected frame | dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation); dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:384` |
| Selected objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:837` |
| Set | Animation ▸ Motion-sync Settings…: mapping row | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:605` |
| Set Random Pose | Modeling ▸ Set Random Pose (model); Modeling ▸ Set Random Pose (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1391` |
| Show | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:947` |
| Show parameter ID and effectiveness in tooltip | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1471` |
| Show parameter name and effectiveness in tooltip | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1470` |
| Skip invisible objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:845` |
| Skip locked objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:844` |
| Still images (JPEG) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… ▸ GIF (animation) | yes | `crates/keyform-app/src/export_media.rs:359` |
| Still images (PNG) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… ▸ GIF (animation) | yes | `crates/keyform-app/src/export_media.rs:354` |
| Stop | Animation ▸ Motion-sync Settings…: audio preview playing | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:860` |
| Subtools | Mesh Edit ▸ Subtools (mesh edit) | yes | `crates/keyform-app/src/app.rs:1498` |
| Synchronize left and right side setting values | Modeling ▸ face generator dialog (Auto Generation of Face Motion…) | by hand | `crates/keyform-app/src/generators_ui.rs:138` |
| Synthesize corners… | Modeling ▸ Parameter ▸ Synthesize corners… (model); Modeling ▸ Parameter ▸ Synthesize corners… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1211` |
| Target follow | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:600` |
| Texture Atlas | Modeling ▸ Texture Atlas (model); dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/app.rs:1301` |
| Top left | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:813` |
| Top right | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:814` |
| Track cursor | Animation ▸ Track cursor (no document); Animation ▸ Track cursor (model) | yes | `crates/keyform-app/src/app.rs:1555` |
| Unbind | As Bind | by hand | `crates/keyform-app/src/app.rs:3173` |
| Unbind Glue Vertices | Modeling ▸ Glue ▸ Unbind Glue Vertices (model); Modeling ▸ Glue ▸ Unbind Glue Vertices (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1165` |
| Undo | Edit ▸ Undo (no document); Edit ▸ Undo (model) | yes | `crates/keyform-app/src/app.rs:895` |
| Unlink | Modeling ▸ Physics Settings…: linked-animation menu, after "Link animation file…" linked a file | by hand | `crates/keyform-app/src/physics_editor.rs:693` |
| Unselect all | Animation workspace ▸ Scene ▸ Bake Animation from Physics… | by hand | `crates/keyform-app/src/animation.rs:1502` |
| Up | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Up (model) | yes | `crates/keyform-app/src/physics_editor.rs:1068` |
| Update keyform | Generator dialogs (sway / 3D rotation / face): apply button | by hand | `crates/keyform-app/src/generators_ui.rs:151` |
| Use Center of Canvas | Modeling ▸ Edit Form ▸ Motion Mirroring…: mirror axis choice (disabled when only deformers are selected) | by hand | `crates/keyform-app/src/form_dialogs.rs:411` |
| Video (MP4) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ Image sequence / GIF / Video… ▸ GIF (animation) | yes | `crates/keyform-app/src/export_media.rs:365` |
| Viewer | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1617` |
| Visible model images | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Add texture… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:305` |
| Whole model images (not only the mesh bounds) | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:228` |
| Window | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1036` |
| Yes | Closing a modified document (File ▸ Close): save prompt | by hand | `crates/keyform-app/src/app.rs:500` |
| Zoom In | Show ▸ Zoom In (no document); Show ▸ Zoom In (model) | yes | `crates/keyform-app/src/app.rs:985` |
| Zoom Out | Show ▸ Zoom Out (no document); Show ▸ Zoom Out (model) | yes | `crates/keyform-app/src/app.rs:997` |
| — | dialog via Modeling ▸ Auto Generation of Face Motion… (model); Auto Generation of Face Motion dialog via Modeling ▸ Auto Generation of Face Motion… ▸ — (model) | yes | `crates/keyform-app/src/template_dialog.rs:146` |
| ⏹ | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1904` |
| ☰ | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/palettes.rs:130` |
| ✕ | Animation workspace ▸ Events menu: delete button next to each event (after "Add at frame …") | by hand | `crates/keyform-app/src/animation.rs:1725` |

## Shortcuts

| Feature | How to reach it | Checked | Code |
|---|---|---|---|
| A | shown in the tooltip of Arrow | yes | `crates/keyform-app/src/app.rs:2232` |
| Backspace | Same as Delete (Edit ▸ Delete shows "Delete"); for keyboards without a Delete key | by hand | `crates/keyform-app/src/app.rs:2188` |
| Ctrl+4 | shown on Modeling ▸ Parameter ▸ Synthesize corners… | yes | `crates/keyform-app/src/app.rs:2281` |
| Ctrl+A | shown on Edit ▸ Select All | yes | `crates/keyform-app/src/app.rs:2367` |
| Ctrl+E | shown on Modeling ▸ ArtMesh ▸ Mesh edit | yes | `crates/keyform-app/src/app.rs:2343` |
| Ctrl+G | shown on Modeling ▸ Glue ▸ Generate and Register Glue | yes | `crates/keyform-app/src/app.rs:2198` |
| Ctrl+N | shown on File ▸ New ▸ Model | yes | `crates/keyform-app/src/app.rs:2315` |
| Ctrl+R | shown on Mesh Edit ▸ Auto attach Edge (line) | yes | `crates/keyform-app/src/app.rs:2333` |
| Ctrl+S | shown on File ▸ Save | yes | `crates/keyform-app/src/app.rs:2177` |
| Ctrl+Shift+A | shown on Modeling ▸ ArtMesh ▸ Automatic Mesh generator… | yes | `crates/keyform-app/src/app.rs:2327` |
| Ctrl+Shift+B | shown on Modeling ▸ Edit Form ▸ Blend Form… | yes | `crates/keyform-app/src/app.rs:2280` |
| Ctrl+Shift+C | shown on Modeling ▸ Edit Form ▸ Copy Form | yes | `crates/keyform-app/src/app.rs:2278` |
| Ctrl+Shift+G | shown on Modeling ▸ Glue ▸ Unbind Glue Vertices | yes | `crates/keyform-app/src/app.rs:2196` |
| Ctrl+Shift+S | shown on File ▸ Save as… | yes | `crates/keyform-app/src/app.rs:2176` |
| Ctrl+Shift+V | shown on Modeling ▸ Edit Form ▸ Paste Form | yes | `crates/keyform-app/src/app.rs:2279` |
| Ctrl+Shift+Z | Second Redo shortcut (macOS order, menus.md); Edit ▸ Redo shows Ctrl+Y | by hand | `crates/keyform-app/src/app.rs:2385` |
| Ctrl+T | shown on Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… | yes | `crates/keyform-app/src/app.rs:2321` |
| Ctrl+Y | shown on Edit ▸ Redo | yes | `crates/keyform-app/src/app.rs:2384` |
| Ctrl+Z | shown on Edit ▸ Undo | yes | `crates/keyform-app/src/app.rs:2386` |
| Delete | shown on Edit ▸ Delete | yes | `crates/keyform-app/src/app.rs:2188` |
| Escape | shown on Edit ▸ Deselect | yes | `crates/keyform-app/src/app.rs:2368` |
| L | shown in the tooltip of Lasso | yes | `crates/keyform-app/src/app.rs:2233` |
| P | shown in the tooltip of Path | yes | `crates/keyform-app/src/app.rs:2235` |

## Menu items never enabled

| Feature | How to reach it | Checked | Code |
|---|---|---|---|
| Edit ▸ Redo | Enabled after an Undo (Edit ▸ Undo, Ctrl+Z); the audit's states have no undo history | by hand | |
| Mesh Edit ▸ Delete Vertex | In mesh edit with vertices selected (click or rubber-band vertices first) | by hand | |
| Mesh Edit ▸ Mirror Edit ▸ Center of Selected Vertices | In mesh edit with vertices selected | by hand | |
| Modeling ▸ Edit Form ▸ Blend Form… | Enabled after Edit Form ▸ Copy Form (Ctrl+Shift+C), like Paste Form | by hand | |
| Modeling ▸ Edit Form ▸ Paste Form | Enabled after Edit Form ▸ Copy Form (Ctrl+Shift+C) with a mesh or deformer selected | by hand | |

## Spec menu items not in Keyform's menus

From `keyform-spec/50-ui/menus.md`; unimplemented or named differently.

### In another place in Keyform

- File ▸ External Application Integration Settings → Show ▸ External Application Integration Settings…
- Show ▸ Texture Atlas → Modeling ▸ Texture Atlas
- Modeling ▸ Parameter ▸ Auto Generation of Face Motion → Modeling ▸ Auto Generation of Face Motion…
- Modeling ▸ Parameter ▸ Auto Generation of Sway Motion → Modeling ▸ Auto Generation of Sway Motion…
- Modeling ▸ Parameter ▸ Apply 3D Rotation Expression → Modeling ▸ Apply 3D Rotation Expression…
- Modeling ▸ Parameter ▸ Motion-sync Settings → Modeling ▸ Motion-sync Settings…
- Form Animation ▸ Deformer → Modeling ▸ Deformer
- Window ▸ Workspace ▸ Reset Current Layout → Window ▸ Reset Current Layout
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Zoom out → Show ▸ Zoom Out
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Zoom in → Show ▸ Zoom In
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Set Random Pose → Modeling ▸ Set Random Pose
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details → Window ▸ Tool Details
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Edit Glue ▸ Weight brush A:B → Modeling ▸ Glue ▸ Weight brush A:B
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Edit Glue ▸ Weight brush A → Modeling ▸ Glue ▸ Weight brush A
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Edit Glue ▸ Weight brush B → Modeling ▸ Glue ▸ Weight brush B
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Edit Glue ▸ Re-merge Glue Vertices Tool → Modeling ▸ Glue ▸ Re-merge Glue Vertices Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Brush Tool ▸ Deform brushes → Mesh Edit ▸ Subtools ▸ Deform brushes
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ New Part → Modeling ▸ Part ▸ New Part
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Flip → Modeling ▸ Edit Form ▸ Flip…
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Add 2 keys → context menu of a Parameter row ▸ Add 2 keys
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Add 3 keys → context menu of a Parameter row ▸ Add 3 keys
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Delete all keys → context menu of a Parameter row ▸ Delete all keys
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Edit Parameter → context menu of a Parameter row ▸ Edit Parameter
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Create Parameter → context menu of a Parameter row ▸ Create Parameter
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Delete → Edit ▸ Delete
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Save → File ▸ Save
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Save ▸ Save → File ▸ Save
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Save ▸ Save → File ▸ Save
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Save ▸ Save → File ▸ Save
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Save ▸ Save → File ▸ Save
- Mesh Edit mode ▸ Finish Mesh editing → Mesh Edit ▸ Finish Mesh editing
- Mesh Edit mode ▸ Automatically Generate Meshes(Mesh Editing Mode) → Mesh Edit ▸ Automatically Generate Meshes(Mesh Editing Mode)
- Mesh Edit mode ▸ Reset Mesh → Mesh Edit ▸ Reset Mesh
- Mesh Edit mode ▸ Merge Vertices → Mesh Edit ▸ Merge Vertices
- Mesh Edit mode ▸ Subtools → Mesh Edit ▸ Subtools
- Mesh Edit mode ▸ Subtools ▸ Select Arrow → Mesh Edit ▸ Subtools ▸ Select Arrow
- Mesh Edit mode ▸ Subtools ▸ Deform brushes → Mesh Edit ▸ Subtools ▸ Deform brushes
- Mesh Edit mode ▸ Subtools ▸ Add Point Tool → Mesh Edit ▸ Subtools ▸ Add Point Tool
- Mesh Edit mode ▸ Subtools ▸ Delete Vertex → Mesh Edit ▸ Delete Vertex
- Mesh Edit mode ▸ Subtools ▸ Eraser → Mesh Edit ▸ Subtools ▸ Eraser

### Not in Keyform's menus

- File ▸ Open Recent File [Ctrl+Shift+O]
- File ▸ Export Image/Video ▸ GIF Animation
- File ▸ Export Image/Video ▸ Still Image [Ctrl+I]
- File ▸ Export Image/Video ▸ PSD images(beta) [Ctrl+P]
- File ▸ Export Image/Video ▸ Video [Ctrl+Shift+M]
- File ▸ Export For Runtime ▸ Export Physics settings
- File ▸ Export For Runtime ▸ Export motion file [Ctrl+Alt+Shift+S]
- File ▸ Environment Settings
- File ▸ Keyboard Shortcuts [Ctrl+Alt+Shift+K]
- File ▸ Model Statistics
- File ▸ Canvas Settings
- File ▸ Resize Model
- Edit ▸ Cut [Ctrl+X]
- Edit ▸ Copy [Ctrl+C]
- Edit ▸ Paste [Ctrl+V]
- Edit ▸ Copy Parameter Value
- Edit ▸ Paste Parameter Value
- Edit ▸ Deselect Hidden and Locked Objects
- Edit ▸ Search and Replace [Ctrl+F]
- Show ▸ Grid
- Show ▸ Grid ▸ Show Grid
- Show ▸ Grid ▸ Grid settings
- Show ▸ Guide
- Show ▸ Guide ▸ Show Guide
- Show ▸ Guide ▸ Show Guide
- Show ▸ Guide ▸ Add Guide to Center
- Show ▸ Guide ▸ Add Guides at Coordinates of Selected Vertices
- Show ▸ Guide ▸ Guide Settings
- Show ▸ Guide ▸ Snap Moving Vertices to Guide
- Show ▸ Show ArtMesh vertex
- Show ▸ Show ArtMesh's Vertex Number
- Show ▸ Highlight vertices outside parent deformer
- Show ▸ Highlight Deformer's Child Elements [Ctrl+Shift+D]
- Show ▸ Highlight overlapping polygons
- Show ▸ Show Child Elements When Resize Warp Deformer
- Show ▸ Show Object Name at Cursor Position
- Show ▸ Hide selected state
- Show ▸ Focus on Selected Object [F]
- Show ▸ Model Image
- Show ▸ Display Quality
- Show ▸ Display Quality ▸ Full Scale
- Show ▸ Display Quality ▸ 1/2
- Show ▸ Display Quality ▸ 1/4
- Show ▸ Display Quality ▸ 1/8
- Show ▸ Show/Hide Bounding Box
- Show ▸ Onion skin
- Show ▸ Onion skin ▸ Modeling View
- Show ▸ Onion skin ▸ Animation View
- Show ▸ Onion skin ▸ Form Animation View
- Show ▸ Onion skin ▸ Onion skin setting
- Show ▸ Track Trajectory
- Show ▸ Track Trajectory ▸ Animation View
- Show ▸ Track Trajectory ▸ Track Trajectory Settings
- Modeling ▸ Deformer ▸ Auto Generation of Deformer
- Modeling ▸ Deformer ▸ Select Deformer for Parent [Ctrl+D]
- Modeling ▸ Deformer ▸ Select Deformer's Child Elements [Ctrl+Alt+D]
- Modeling ▸ Deformer ▸ Delete Deformer and its Content [Ctrl+Delete]
- Modeling ▸ Deformer ▸ Apply Deformer to Child Elements
- Modeling ▸ Deformer ▸ Validate Deformer
- Modeling ▸ Deformer ▸ Align Angle of Bounding Box
- Modeling ▸ Parameter ▸ Reset to default values [Ctrl+1]
- Modeling ▸ Parameter ▸ Lock Default Form [Ctrl+Shift+1]
- Modeling ▸ Parameter ▸ Synchronize Parameter Values Across Tabs
- Modeling ▸ Parameter ▸ Parameter settings
- Modeling ▸ Parameter ▸ Parameter Group Settings
- Modeling ▸ Parameter ▸ Set All Parameter Values as Default
- Modeling ▸ Parameter ▸ Auto Generation of Face Motion ▸ Generate Face Deformer
- Modeling ▸ Parameter ▸ Auto Generation of Face Motion ▸ Generate Face Motion
- Modeling ▸ Parameter ▸ Multiple Keys Editing
- Modeling ▸ Parameter ▸ Select Object Linked to Key
- Modeling ▸ Parameter ▸ Arrange Parameter Configuration for Selected Object
- Modeling ▸ Parameter ▸ Settings for Eye Blinking and Lip-sync
- Modeling ▸ Parameter ▸ Validate Assigned Parameters
- Modeling ▸ ArtMesh ▸ Change Draw Order of All ArtMeshes
- Modeling ▸ ArtMesh ▸ Bulk Change Draw Order of ArtMeshes
- Modeling ▸ ArtMesh ▸ Create artMesh for collision detection
- Modeling ▸ ArtPath
- Modeling ▸ ArtPath ▸ Automatic generation from a single ArtMesh
- Modeling ▸ ArtPath ▸ Automatic generation from multiple ArtMeshes
- Modeling ▸ ArtPath ▸ Apply Color
- Modeling ▸ ArtPath ▸ Apply Opacity
- Modeling ▸ ArtPath ▸ Apply Line Width
- Modeling ▸ ArtPath ▸ Apply All
- Modeling ▸ ArtPath ▸ Align with Outline ArtMesh
- Modeling ▸ ArtPath ▸ Align with Fill ArtMesh
- Modeling ▸ ArtPath ▸ Add control points to align with ArtMesh
- Modeling ▸ ArtPath ▸ Add a point in the middle of all control points
- Modeling ▸ ArtPath ▸ Add a point in the middle of the selected control point
- Modeling ▸ ArtPath ▸ Open Auto Generation ArtPath Dialog
- Modeling ▸ ArtPath ▸ Create DeformPath from ArtPath
- Modeling ▸ ArtPath ▸ Bake ArtMesh deformation to ArtPath
- Modeling ▸ ArtPath ▸ Follow all
- Modeling ▸ ArtPath ▸ Cancel all
- Modeling ▸ ArtPath ▸ Follow selected control points
- Modeling ▸ ArtPath ▸ Release selected control point
- Modeling ▸ ArtPath ▸ Open ArtPath Brush Settings Dialog
- Modeling ▸ Glue ▸ Glue On/Off
- Modeling ▸ Skinning
- Modeling ▸ Skinning ▸ Skinning
- Modeling ▸ Skinning ▸ Use deformation path for Skinning
- Modeling ▸ Skinning ▸ Generate parameters for rotation deformer
- Modeling ▸ Edit Form ▸ Confirm target when pasting multiple forms
- Modeling ▸ Edit Form ▸ Zoom In / Zoom Out Specifying Magnification
- Modeling ▸ Edit Form ▸ Shape Warp Deformer along Bezier Handle
- Modeling ▸ Edit Form ▸ Expand Warp Deformer Based on Children Outside Range
- Modeling ▸ Edit Form ▸ Revert to Original
- Modeling ▸ Edit Form ▸ Update Original Shape of Deformers
- Modeling ▸ Edit Form ▸ Delete Original Shape of All Deformers
- Modeling ▸ Temporary deform tool
- Modeling ▸ Temporary deform tool ▸ Enlarge/shrink
- Modeling ▸ Temporary deform tool ▸ Rotation
- Modeling ▸ Temporary deform tool ▸ Free transform
- Modeling ▸ Temporary deform tool ▸ Perspective
- Modeling ▸ Temporary deform tool ▸ Distort
- Modeling ▸ Temporary deform tool ▸ Temporary warp deformation
- Modeling ▸ Temporary deform tool ▸ Add columns
- Modeling ▸ Temporary deform tool ▸ Remove columns
- Modeling ▸ Temporary deform tool ▸ Add rows
- Modeling ▸ Temporary deform tool ▸ Remove rows
- Modeling ▸ Temporary deform tool ▸ Temporary path deformation
- Modeling ▸ Temporary deform tool ▸ Deselect the temporary deform tool
- Modeling ▸ Clipping ID
- Modeling ▸ Clipping ID ▸ Select Clipping Mask
- Modeling ▸ Clipping ID ▸ Reverse Clipping ID Lookup
- Modeling ▸ Model Template ▸ Apply Template
- Modeling ▸ Model Template ▸ Export template
- Modeling ▸ Texture Atlas ▸ Switch between Model Image and Texture Atlas [T]
- Modeling ▸ Texture Atlas ▸ Apply Texture Atlas to Model Image
- Modeling ▸ Lock
- Modeling ▸ Lock ▸ Lock/unlock Selected Objects [Ctrl+2]
- Modeling ▸ Lock ▸ Lock the unselected objects [Ctrl+Shift+2]
- Modeling ▸ Lock ▸ Unlock all [Ctrl+Alt+2]
- Modeling ▸ Show/Hide
- Modeling ▸ Show/Hide ▸ Show/Hide Selected Objects [Ctrl+3]
- Modeling ▸ Show/Hide ▸ Hide the unselected objects [Ctrl+Shift+3]
- Modeling ▸ Show/Hide ▸ Show all [Ctrl+Alt+3]
- Modeling ▸ Show/Hide ▸ Show/Hide Guide Image [Ctrl+Alt+Shift+3]
- Modeling ▸ Show/Hide ▸ Restore to Previous Visibility State
- Modeling ▸ Edit Level
- Modeling ▸ Edit Level ▸ Level 1 [1]
- Modeling ▸ Edit Level ▸ Level 2 [2]
- Modeling ▸ Edit Level ▸ Level 3 [3]
- Modeling ▸ Convert Model ID
- Modeling ▸ Bulk setting of model objects
- Modeling ▸ Bulk setting of model objects ▸ Export
- Modeling ▸ Bulk setting of model objects ▸ Import
- Modeling ▸ Bulk setting of model parameters
- Modeling ▸ Bulk setting of model parameters ▸ Export
- Modeling ▸ Bulk setting of model parameters ▸ Import
- Animation ▸ Material
- Animation ▸ Material ▸ Reload Material of Selected Track
- Animation ▸ Material ▸ Replace Material of Selected Track
- Animation ▸ Material ▸ Replace Material of Selected Track
- Animation ▸ Scene
- Animation ▸ Scene ▸ New Scene
- Animation ▸ Scene ▸ Import Scene
- Animation ▸ Scene ▸ Generate Scenes Automatically from Audio Files
- Animation ▸ Scene ▸ Enable/Disable Loop Editing Support
- Animation ▸ Track
- Animation ▸ Track ▸ Bake Animation from Parameter Controller
- Animation ▸ Track ▸ Bake Animation from Physics
- Animation ▸ Track ▸ Bake Animation from Motion-sync
- Animation ▸ Track ▸ Apply Lip-sync from the Audio File
- Animation ▸ Track ▸ Start Track at Current Position [Shift+B]
- Animation ▸ Track ▸ End Track at Current Position [Shift+N]
- Animation ▸ Track ▸ Move Track to Current Position [Shift+M]
- Animation ▸ Timeline
- Animation ▸ Timeline ▸ Move to Beginning [Home]
- Animation ▸ Timeline ▸ Move to End [End]
- Animation ▸ Timeline ▸ Move to Start Frame of Work Area [Shift+Home]
- Animation ▸ Timeline ▸ Move to End Frame of Work Area [Shift+End]
- Animation ▸ Timeline ▸ Previous frame [Page Up]
- Animation ▸ Timeline ▸ Next frame [Page Down]
- Animation ▸ Timeline ▸ Previous Keyframe [J]
- Animation ▸ Timeline ▸ Next Keyframe [K]
- Animation ▸ Timeline ▸ Previous Keyframe in Track [Shift+J]
- Animation ▸ Timeline ▸ Next Keyframe in Track [Shift+K]
- Animation ▸ Timeline ▸ Return to Previous Marker [Ctrl+J]
- Animation ▸ Timeline ▸ Proceed to Next Marker [Ctrl+K]
- Animation ▸ Timeline ▸ Return to the previous onion skin marker [Ctrl+Shift+J]
- Animation ▸ Timeline ▸ Go to the next onion skin marker [Ctrl+Shift+K]
- Animation ▸ Timeline ▸ Set as Start Frame of Work Area
- Animation ▸ Timeline ▸ Set as End Frame of Work Area
- Animation ▸ Timeline ▸ Display Entire Work Area [Shift+F]
- Animation ▸ Timeline ▸ Display Entire Scene
- Animation ▸ Timeline ▸ Reset Zoom
- Animation ▸ Fade
- Animation ▸ Fade ▸ Set fade-in/fade-out
- Animation ▸ Fade ▸ Apply Fade-In/Fade-Out Settings to Other Scenes
- Animation ▸ Parameter Controller
- Animation ▸ Parameter Controller ▸ Parameter Controller Calculation Unit Settings
- Animation ▸ Parameter Controller ▸ Reverse Direction Controller
- Animation ▸ Parameter Controller ▸ Convert Direction Controller into 2 Single Point Controllers
- Animation ▸ Parameter Controller ▸ Convert 2 Single Point Controllers into Direction Controller
- Animation ▸ Parameter Controller ▸ Follow Target
- Animation ▸ Parameter Controller ▸ Calculate collectively before playback
- Animation ▸ Parameter Controller ▸ Show Controller
- Animation ▸ Parameter Controller ▸ Controller Display Settings
- Animation ▸ Parameter Bookmarks
- Animation ▸ Parameter Bookmarks ▸ Display with Selected Parameters
- Animation ▸ Parameter Bookmarks ▸ Delete Labels from Selected Tracks
- Animation ▸ Parameter Bookmarks ▸ Duplicate All Labels
- Animation ▸ Parameter Bookmarks ▸ Duplicate Labels Individually
- Animation ▸ Parameter Bookmarks ▸ Hide All Labels
- Animation ▸ Parameter Bookmarks ▸ Parameter Bookmark Display Settings
- Animation ▸ Add Marker
- Animation ▸ Playback [Enter]
- Animation ▸ Switch Edit Mode [C]
- Animation ▸ Gather files
- Form Animation ▸ Deformer ▸ Select Deformer for Parent [Ctrl+D]
- Form Animation ▸ Deformer ▸ Select Deformer's Child Elements [Ctrl+Alt+D]
- Form Animation ▸ Temporary deform tool
- Form Animation ▸ Temporary deform tool ▸ Enlarge/shrink
- Form Animation ▸ Temporary deform tool ▸ Rotation
- Form Animation ▸ Temporary deform tool ▸ Free transform
- Form Animation ▸ Temporary deform tool ▸ Perspective
- Form Animation ▸ Temporary deform tool ▸ Distort
- Form Animation ▸ Temporary deform tool ▸ Temporary warp deformation
- Form Animation ▸ Temporary deform tool ▸ Add columns
- Form Animation ▸ Temporary deform tool ▸ Remove columns
- Form Animation ▸ Temporary deform tool ▸ Add rows
- Form Animation ▸ Temporary deform tool ▸ Remove rows
- Form Animation ▸ Temporary deform tool ▸ Temporary path deformation
- Form Animation ▸ Temporary deform tool ▸ Deselect the temporary deform tool
- Form Animation ▸ Clipping ID
- Form Animation ▸ Clipping ID ▸ Select Clipping Mask
- Form Animation ▸ Clipping ID ▸ Reverse Clipping ID Lookup
- Form Animation ▸ Lock
- Form Animation ▸ Lock ▸ Lock/unlock Selected Objects [Ctrl+2]
- Form Animation ▸ Lock ▸ Lock the unselected objects [Ctrl+Shift+2]
- Form Animation ▸ Lock ▸ Unlock all [Ctrl+Alt+2]
- Form Animation ▸ Show/Hide
- Form Animation ▸ Show/Hide ▸ Show/Hide Selected Objects [Ctrl+3]
- Form Animation ▸ Show/Hide ▸ Hide the unselected objects [Ctrl+Shift+3]
- Form Animation ▸ Show/Hide ▸ Show all [Ctrl+Alt+3]
- Form Animation ▸ Show/Hide ▸ Restore to Previous Visibility State
- Form Animation ▸ Edit Level
- Form Animation ▸ Edit Level ▸ Level 1 [1]
- Form Animation ▸ Edit Level ▸ Level 2 [2]
- Form Animation ▸ Edit Level ▸ Level 3 [3]
- Form Animation ▸ Create form edit key
- Form Animation ▸ Animation preview setting
- Window ▸ Workspace
- Window ▸ Workspace ▸  [CMenuItem]
- Window ▸ Workspace ▸ Update Initial Layout
- Window ▸ Workspace ▸ Workspace Settings
- Window ▸ Hide Palette
- Window ▸ Show Palette
- Window ▸ Duplicate Tab
- Window ▸ Switch to Previous Tab
- Window ▸ Switch to Next Tab
- Window ▸ Set All Views to Same Display Position
- Window ▸ Reset Status
- Help ▸ User Manual
- Help ▸ Go to Live2D Web Page
- Help ▸ Product Page
- Help ▸ Download Page
- Help ▸ Sample Model
- Help ▸ Creators Forum
- Help ▸ Live2D Help
- Help ▸ Tutorial Video
- Help ▸ Home
- Help ▸ Display Operation
- Help ▸ Open Log File
- Help ▸ Open Auto Backup Folder
- Help ▸ Update History
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Rotation Deformer Creation Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Arrow tool [A]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Lasso Tool [L]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Brush Selection tool [B]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Deform Path edit [P]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Deform Brush Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Edit Glue
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ ArtPath Tools
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Switch to Last Used Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Switch Workspace to Modeling
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Switch Workspace to Animation
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Switch workspace to Form Animation
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Lock ArtMesh [Shift+A]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Lock Deformer [Shift+D]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Show/hide ArtMesh
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Solo [S]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Apply Opacity
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Apply multiply color/screen color
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Select Objects of Selected Vertices
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Snapshot [Ctrl+5]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Show/hide snapshot [5]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Save snapshot guide image [Ctrl+Shift+5]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Display as original size
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Display All [Ctrl+0]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Flip canvas [R]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Play/Stop Random Pose
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Lightweight playback
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Highlight Frame Step frames
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Auto-scroll to Form Animation property of selected object
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Automatically scroll to properties of current parameter controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Enable moving indicator out of scene range
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Enable Operation of Keyframes Hidden by Shy
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Expand all tracks
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Select all keys when selecting parameters
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Exclude Frame Step When Bulk Inserting Keyframes
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Display 1 Frame Range at Indicator
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Display key values
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Display key name
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Auto resize
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Always edit continuous continuous Beziers
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Smart snap using Shift
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Make Beziers by dragging curves
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Snap to key position on curve
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Freeze/Unfreeze at Current Frame
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Freeze All
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Freeze Selected Properties
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Freeze Unselected Properties
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Unfreeze All
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Make art path control points thicker
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Make art path control points thinner
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Snap to Nearest Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Lock Draw Order Slider
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Path edit
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Path edit ▸ Add control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Path edit ▸ Delete control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Path edit ▸ Delete path
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Path edit ▸ Fold line
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Edit Glue
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Add control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Delete control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Adjust the opacity set in the control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Adjust the thickness set in the control point
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Fold line
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Stroke ArtPath
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Re-stroke ArtPath
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Join ArtPaths
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ ArtPath Tools ▸ Split ArtPath
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Brush Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Brush Tool ▸ Expansion Brush
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Deform Brush Tool ▸ Warp Deformer Shaping Brush
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool ▸ Select/Edit Controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool ▸ Add Single Point Controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool ▸ Add Direction Controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool ▸ Delete Controller
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Tool Details ▸ Parameter Controller Tool ▸ Edit Target
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Add Guide
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Add Guide ▸ Add a Horizontal Line to Mouse Position
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Add Guide ▸ Add a Vertical Line to Mouse Position
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Add Guide ▸ Add Horizontal/Vertical Lines to Mouse Position
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Palette Common Shortcuts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Palette Common Shortcuts ▸ Select for Movement
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Palette Common Shortcuts ▸ Move Here
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Delete selected Parts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Delete selected Parts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Disable drag & drop
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Part Settings
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Delete empty parts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Delete empty parts below child
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Part Shortcuts ▸ Delete empty deformers
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Deformer Shortcuts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Deformer Shortcuts ▸ Disable drag & drop
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Deformer Shortcuts ▸ Delete empty deformers below child
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Cancel Selection
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Combine/uncombine
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Previous
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Next
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select First Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Next Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Previous Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Last Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Default Key
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Fist Vertical Key(combined)
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Next Vertical Key(combined)
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Previous Vertical Key(combined)
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select Last Vertical Key(combined)
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Add Single Keyform
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Delete Keyform
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Select
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Adjust
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Change
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Edit keyform
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Display with Parameters of Selected Objects Only
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Replicate parameters
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Create folder
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Delete empty parameters
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Delete empty parameter groups
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Shortcuts ▸ Disable drag & drop
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Show
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Show ▸ Show [1]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Show ▸ Show [2]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Show ▸ Show [3]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Temporary Snapshots ▸ Show ▸ Show [4]
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Column operations for temporary warp deformations
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Column operations for temporary warp deformations ▸ Add columns
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Column operations for temporary warp deformations ▸ Remove columns
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Column operations for temporary warp deformations ▸ Add rows
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Column operations for temporary warp deformations ▸ Remove rows
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Blend Shape
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Bookmarks
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Bookmarks ▸ Display with Object at Cursor Position
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Parameter Bookmarks ▸ Select from Objects at Cursor Position
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Brush
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Brush ▸ Switch to Previous Brush Shape
- Keyboard-only commands (no menu entry; reachable by shortcut / command palette) ▸ Brush ▸ Switch to Next Brush Shape
- Mesh Edit mode ▸ Auto attach Edge [Ctrl+R]
- Mesh Edit mode ▸ Paste with Copied Shape [Ctrl+Alt+V]
- Mesh Edit mode ▸ Flip horizontally
- Mesh Edit mode ▸ Flip vertically
- Mesh Edit mode ▸ Invert Axis
- Mesh Edit mode ▸ Glue / bind the selected vertices [Ctrl+G]
- Mesh Edit mode ▸ Unglue / unbind the selected vertices [Ctrl+Shift+G]
- Mesh Edit mode ▸ Stroke mesh-mapping
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Increase mesh width
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Reduce mesh width
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Increase divisions
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Reduce divisions
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Change the number of vertices in the mesh width
- Mesh Edit mode ▸ Stroke mesh-mapping ▸ Confirm stroke [Shift+E]
- Mesh Edit mode ▸ Subtools ▸ Lasso
- Mesh Edit mode ▸ Subtools ▸ Add points
- Mesh Edit mode ▸ Subtools ▸ Stroke mesh-mapping
