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
| (none) | File ▸ Recent Files ▸ (none) (no document); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/form_dialogs.rs:572` |
| 4 Division | Mesh Edit mode ▸ 4 Division (mesh edit) | yes | `crates/keyform-app/src/app.rs:1909` |
| < Add all layers as new ArtMesh > | File ▸ Open a PSD with a model open: the PSD dialog lists these first (session step 1 uses the PSD flow without a model) | by hand | `crates/keyform-app/src/psd_dialog.rs:173` |
| < Register PSD file without adding ArtMeshes > | As above (PSD dialog with a model open) | by hand | `crates/keyform-app/src/psd_dialog.rs:177` |
| Add | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1134` |
| Add a number to the names of pasted objects | Environment Settings dialog via File ▸ Environment Settings… ▸ Modeling (no document) | yes | `crates/keyform-app/src/env_settings.rs:182` |
| Add control point | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:779` |
| Add group | Model Settings (runtime model) ▸ Pose section | by hand | `crates/keyform-app/src/viewer_settings.rs:478` |
| Add Guide to Center | Show ▸ Guide ▸ Add Guide to Center (no document); Show ▸ Guide ▸ Add Guide to Center (model) | yes | `crates/keyform-app/src/view_aids.rs:510` |
| Add HitArea meshes | Model Settings (runtime model) ▸ Hit Areas section | by hand | `crates/keyform-app/src/viewer_settings.rs:519` |
| Add image track… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1849` |
| Add key at playhead | Animation workspace: select an attribute row in the timeline, the curve editor shows it (session step: scene keys) | by hand | `crates/keyform-app/src/animation.rs:2307` |
| add line automatically | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:800` |
| Add mask… | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ Face (model) | yes | `crates/keyform-app/src/palettes.rs:1250` |
| Add model track | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1816` |
| Add motions… | Model Settings tab (runtime model); Log tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:154` |
| Add parameter | Model Settings (runtime model) ▸ Expressions, editing an expression | by hand | `crates/keyform-app/src/viewer_settings.rs:397` |
| Add parameter… | Animation ▸ Motion-sync Settings…: mapping | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:677` |
| Add scene track | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1840` |
| Add Single Point Controller | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:584` |
| Add sound track (WAV)… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1821` |
| Add texture… | dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:173` |
| Add to the current animation file | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:663` |
| Add… | dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… (model); dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:311` |
| Adjust to position and shape | As above | by hand | `crates/keyform-app/src/template_dialog.rs:130` |
| Adjust to template | As above: blend-mode handling | by hand | `crates/keyform-app/src/template_dialog.rs:163` |
| Adjustment Mode 1 | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1913` |
| Adjustment Mode 2 | combo box 'Adjustment Mode 1' (animation); Deformer ▸ Adjustment Mode 1 (animation) | yes | `crates/keyform-app/src/animation.rs:1915` |
| All deformers | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:839` |
| All drawables | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:834` |
| All keyforms (not only the current one) | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:732` |
| All model images | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Add texture… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:310` |
| All objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:829` |
| Allow 90° rotation | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:225` |
| Alpha Blend | As above | by hand | `crates/keyform-app/src/template_dialog.rs:171` |
| Always show warnings regarding masks | Show ▸ Always show warnings regarding masks (no document); Environment Settings dialog via File ▸ Environment Settings… ▸ Notification (no document) | yes | `crates/keyform-app/src/app.rs:1232` |
| Angle | the toolbar ▸ Soft (model); Create Rotation Deformer dialog via Modeling ▸ Deformer ▸ Create rotation deformer… ▸ Create (model) | yes | `crates/keyform-app/src/physics_editor.rs:1063` |
| Animation | Window ▸ Workspace ▸ Animation (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1311` |
| Animation… | File ▸ New ▸ Animation… (no document); File ▸ New ▸ Animation… (model) | yes | `crates/keyform-app/src/app.rs:895` |
| Apply | Animation ▸ Motion-sync Settings… | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:539` |
| Apply 3D Rotation Expression… | Modeling ▸ Parameter ▸ Apply 3D Rotation Expression… (model); Modeling ▸ Parameter ▸ Apply 3D Rotation Expression… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1564` |
| Apply Expression | Animation ▸ Apply Expression (no document); Animation ▸ Apply Expression (model) | yes | `crates/keyform-app/src/app.rs:2022` |
| Apply form edit | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1918` |
| Apply Physics | Animation ▸ Apply Physics (no document); Animation ▸ Apply Physics (model) | yes | `crates/keyform-app/src/app.rs:2024` |
| Apply Template… | Modeling ▸ Model Template ▸ Apply Template… (model); Modeling ▸ Model Template ▸ Apply Template… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1701` |
| Approved | File ▸ External Application Integration Settings…: one checkbox per connected application (none connect in the audit) | by hand | `crates/keyform-app/src/external_host.rs:320` |
| ArtMesh | Modeling ▸ ArtMesh (model); Modeling ▸ ArtMesh (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1405` |
| Audio Elements | Animation ▸ Motion-sync Settings…: tab | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:453` |
| Auto attach Edge | Mesh Edit mode ▸ Auto attach Edge (mesh edit) | yes | `crates/keyform-app/src/app.rs:1886` |
| Auto Generation of Face Motion | Modeling ▸ Parameter ▸ Auto Generation of Face Motion (model); dialog via Modeling ▸ Parameter ▸ Auto Generation of Face Motion ▸ Generate Face Deformer… (model) | yes | `crates/keyform-app/src/app.rs:1542` |
| Auto Generation of Sway Motion… | Modeling ▸ Parameter ▸ Auto Generation of Sway Motion… (model); Modeling ▸ Parameter ▸ Auto Generation of Sway Motion… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1554` |
| Auto Layout… | dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:213` |
| Auto-update maximum of output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1192` |
| Automatic | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:237` |
| Automatic Eye-Blinking | Animation ▸ Automatic Eye-Blinking (no document); Animation ▸ Automatic Eye-Blinking (model) | yes | `crates/keyform-app/src/app.rs:2023` |
| Automatic mapping | As above | by hand | `crates/keyform-app/src/template_dialog.rs:131` |
| Automatic Mesh generator… | Modeling ▸ ArtMesh ▸ Automatic Mesh generator… (model); Modeling ▸ ArtMesh ▸ Automatic Mesh generator… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1429` |
| Axis: horizontal (top ↔ bottom) | Mesh Edit mode ▸ Mirror Edit ▸ Axis: horizontal (top ↔ bottom) (mesh edit) | yes | `crates/keyform-app/src/app.rs:3739` |
| Axis: vertical (left ↔ right) | Mesh Edit mode ▸ Mirror Edit ▸ Axis: vertical (left ↔ right) (mesh edit) | yes | `crates/keyform-app/src/app.rs:3734` |
| Bake Animation from Motion-sync | Animation ▸ Track ▸ Bake Animation from Motion-sync (animation); Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1531` |
| Bake Animation from Parameter Controller | Animation ▸ Track ▸ Bake Animation from Parameter Controller (animation); Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/animation.rs:1515` |
| Bake Animation from Physics… | Animation ▸ Track ▸ Bake Animation from Physics… (animation); Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1523` |
| Bake Lip-sync | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1891` |
| Batch Reflect… | Modeling ▸ Parameter ▸ Batch Reflect… (model); Modeling ▸ Parameter ▸ Batch Reflect… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1573` |
| Bind | Mesh edit of two meshes (select two meshes, Mesh edit): partner binding buttons | by hand | `crates/keyform-app/src/app.rs:3830` |
| Blend Form… | Modeling ▸ Edit Form ▸ Blend Form… (model); Modeling ▸ Edit Form ▸ Blend Form… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1640` |
| Bottom left | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:823` |
| Bottom right | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:824` |
| Breath | Animation ▸ Breath (no document); Animation ▸ Breath (model) | yes | `crates/keyform-app/src/app.rs:2035` |
| Bulk Set | Inspector with several meshes selected: draw order | by hand | `crates/keyform-app/src/inspector.rs:236` |
| Cancel | Home ▸ New Model… (no document); dialog via Show ▸ Grid ▸ Grid settings… (no document) | yes | `crates/keyform-app/src/animation.rs:1455` |
| Cancel (Esc) | Toolbar ▸ Temp path (Tool Details panel), as Confirm | by hand | `crates/keyform-app/src/path_tool.rs:1212` |
| Cancel auto-adjusted culling | dialog via Modeling ▸ Edit Form ▸ Flip… (model); Flip dialog via Modeling ▸ Edit Form ▸ Flip… ▸ Parameters to reflect (model) | yes | `crates/keyform-app/src/form_dialogs.rs:757` |
| Center | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/template_dialog.rs:119` |
| Center of Canvas | Mesh Edit mode ▸ Mirror Edit ▸ Center of Canvas (mesh edit) | yes | `crates/keyform-app/src/app.rs:3751` |
| Center of Selected Vertices | Mesh Edit mode ▸ Mirror Edit ▸ Center of Selected Vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:3757` |
| Centre | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model); Options for Paste Form Special dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… ▸ Centre (model) | yes | `crates/keyform-app/src/form_dialogs.rs:691` |
| Centre (min + max) / 2 | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:811` |
| Clear | Log tab (no document); Tool Details tab (no document) | yes | `crates/keyform-app/src/palettes.rs:1420` |
| Clear recent files | Environment Settings dialog via File ▸ Environment Settings… ▸ File (no document) | yes | `crates/keyform-app/src/env_settings.rs:160` |
| Clipping ID | Modeling ▸ Clipping ID (model); Modeling ▸ Clipping ID (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1668` |
| Close | File ▸ Close (no document); File ▸ Close (model) | yes | `crates/keyform-app/src/app.rs:947` |
| Close all | File ▸ Close all (no document); File ▸ Close all (model) | yes | `crates/keyform-app/src/app.rs:958` |
| Color Blend | As above | by hand | `crates/keyform-app/src/template_dialog.rs:170` |
| Confirm | Animation ▸ Motion-sync Settings… | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:544` |
| Confirm (Enter) | Toolbar ▸ Temp path, then click on the canvas: the Tool Details panel shows Confirm / Cancel | by hand | `crates/keyform-app/src/path_tool.rs:1209` |
| Connect | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3818` |
| Continuous creation | dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model); dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model) ▸ Set as child of selected deformer | yes | `crates/keyform-app/src/create_deformer.rs:163` |
| Controllers | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1936` |
| Copy | Edit ▸ Copy (no document); Edit ▸ Copy (model) | yes | `crates/keyform-app/src/app.rs:1145` |
| Copy Form | Modeling ▸ Edit Form ▸ Copy Form (model); Modeling ▸ Edit Form ▸ Copy Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1615` |
| Copy output lines | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Edit (model) | yes | `crates/keyform-app/src/physics_editor.rs:254` |
| Copy Parameter Value | Edit ▸ Copy Parameter Value (no document); Edit ▸ Copy Parameter Value (model) | yes | `crates/keyform-app/src/app.rs:1173` |
| Create | Home ▸ New Model… (no document); dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… (model) | yes | `crates/keyform-app/src/create_deformer.rs:164` |
| Create a new animation file | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:668` |
| Create new scene | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1797` |
| Create rotation deformer… | Modeling ▸ Deformer ▸ Create rotation deformer… (model); Modeling ▸ Deformer ▸ Create rotation deformer… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1397` |
| Create Warp Deformer… | Modeling ▸ Deformer ▸ Create Warp Deformer… (model); Modeling ▸ Deformer ▸ Create Warp Deformer… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1387` |
| Culling | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ Face (model) | yes | `crates/keyform-app/src/palettes.rs:1197` |
| Cut | Edit ▸ Cut (no document); Edit ▸ Cut (model) | yes | `crates/keyform-app/src/app.rs:1136` |
| Dark | dialog via File ▸ Environment Settings… (no document); Environment Settings dialog via File ▸ Environment Settings… ▸ Dark (no document) | yes | `crates/keyform-app/src/env_settings.rs:131` |
| Dark Theme | Window ▸ Dark Theme (no document); Window ▸ Dark Theme (model) | yes | `crates/keyform-app/src/app.rs:1351` |
| Decrease Output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1152` |
| Decrease priority | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1556` |
| Default Guide Line Color… | Guide Settings dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… ▸ Setting (model) | yes | `crates/keyform-app/src/view_aids.rs:339` |
| Default value | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:810` |
| Deformer | Window ▸ Deformer (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1383` |
| Delete | Edit ▸ Delete (no document); Edit ▸ Delete (model) | yes | `crates/keyform-app/src/app.rs:1163` |
| Delete control point | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:782` |
| Delete Controller | Animation workspace ▸ Controllers menu, with a parameter controller selected | by hand | `crates/keyform-app/src/param_ctrl_ui.rs:634` |
| Delete Form Edit | Animation workspace ▸ Form Edit menu, after a form edit exists for the selected mesh | by hand | `crates/keyform-app/src/animation.rs:1931` |
| Delete key | context menu of a Parameter row ▸ Delete key (model); context menu of a Parameter row ▸ Delete key (model, mesh selected) | yes | `crates/keyform-app/src/animation.rs:2320` |
| Delete path | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:785` |
| Delete scene | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1808` |
| Delete the existing keys and add 3 keys | Modeling ▸ Auto Generation of Sway Motion… (and 3D rotation / face generators) when the chosen parameter doesn't have exactly 3 keys | by hand | `crates/keyform-app/src/generators_ui.rs:112` |
| Delete Vertex | Mesh Edit mode ▸ Delete Vertex (mesh edit) | yes | `crates/keyform-app/src/app.rs:1927` |
| Deselect | Edit ▸ Deselect (no document); Edit ▸ Deselect (model) | yes | `crates/keyform-app/src/app.rs:1211` |
| Disable drag & drop | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:152` |
| Do not show again | Dialog "Issues on Mask Settings" (shown on export when a mask setting needs attention) | by hand | `crates/keyform-app/src/app.rs:2593` |
| Don't change | As above | by hand | `crates/keyform-app/src/template_dialog.rs:164` |
| Don't show again | Dialog "Paste Form: pair copies with targets" (Modeling ▸ Edit Form ▸ Paste Form with several copies) | by hand | `crates/keyform-app/src/form_dialogs.rs:581` |
| Don't show again. | Dialog "About Number of Keys for Extended Interpolation" (choosing extended interpolation for a parameter with fewer than 3 keys) | by hand | `crates/keyform-app/src/form_dialogs.rs:354` |
| Done | Glue tool (Modeling ▸ Glue…): Tool Details panel | by hand | `crates/keyform-app/src/app.rs:4009` |
| Double-click an ArtMesh to start editing its mesh | Environment Settings dialog via File ▸ Environment Settings… ▸ Modeling (no document) | yes | `crates/keyform-app/src/env_settings.rs:169` |
| Down | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Down (model) | yes | `crates/keyform-app/src/physics_editor.rs:1081` |
| Draw order group | Inspector with a part selected | by hand | `crates/keyform-app/src/inspector.rs:445` |
| Duplicate | Motion-sync Settings dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… ▸ Edit… (model); dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:319` |
| Duplicate… | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:798` |
| Edge swap | Mesh Edit mode ▸ Edge swap (mesh edit) | yes | `crates/keyform-app/src/app.rs:1904` |
| Edit | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1104` |
| Edit All Guide Colors… | Guide Settings dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… ▸ Features (model) | yes | `crates/keyform-app/src/view_aids.rs:329` |
| Edit Form | Modeling ▸ Edit Form (model); Modeling ▸ Edit Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1611` |
| Edit Level | Modeling ▸ Edit Level (model); Modeling ▸ Edit Level (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1794` |
| Edit Texture Atlas… | Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model); Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1724` |
| Edit… | dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:314` |
| Enable | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:427` |
| Enable external application integration | dialog via File ▸ External Application Integration Settings… (no document) | yes | `crates/keyform-app/src/external_host.rs:271` |
| Enable Pose Switching | Animation ▸ Enable Pose Switching (no document); Animation ▸ Enable Pose Switching (model) | yes | `crates/keyform-app/src/app.rs:2025` |
| Environment Settings… | File ▸ Environment Settings… (no document); File ▸ Environment Settings… (model) | yes | `crates/keyform-app/src/app.rs:1081` |
| Events | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1941` |
| Exit | File ▸ Exit (no document); File ▸ Exit (model) | yes | `crates/keyform-app/src/app.rs:1098` |
| Export as moc3 file… | File ▸ Export For Runtime ▸ Export as moc3 file… (no document); File ▸ Export For Runtime ▸ Export as moc3 file… (model) | yes | `crates/keyform-app/src/app.rs:1033` |
| Export For Runtime | File ▸ Export For Runtime (no document); File ▸ Export For Runtime (model) | yes | `crates/keyform-app/src/app.rs:1029` |
| Export Image/Video | File ▸ Export Image/Video (no document); File ▸ Export Image/Video (model) | yes | `crates/keyform-app/src/app.rs:991` |
| Export motion file… | File ▸ Export For Runtime ▸ Export motion file… (no document); File ▸ Export For Runtime ▸ Export motion file… (model) | yes | `crates/keyform-app/src/app.rs:1058` |
| Export Parameter Controller… | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:652` |
| Export Physics settings… | File ▸ Export For Runtime ▸ Export Physics settings… (no document); File ▸ Export For Runtime ▸ Export Physics settings… (model) | yes | `crates/keyform-app/src/app.rs:1043` |
| Export template… | Modeling ▸ Model Template ▸ Export template… (model); Modeling ▸ Model Template ▸ Export template… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1711` |
| Extended Interpolation… | Modeling ▸ Parameter ▸ Extended Interpolation… (model); Modeling ▸ Parameter ▸ Extended Interpolation… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1583` |
| Fade | Animation ▸ Fade (no document); Animation ▸ Fade (model) | yes | `crates/keyform-app/src/animation.rs:1543` |
| Features | dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… (model) | yes | `crates/keyform-app/src/view_aids.rs:327` |
| File | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:886` |
| Finish | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:2231` |
| Finish Mesh editing | Mesh Edit mode ▸ Finish Mesh editing (mesh edit) | yes | `crates/keyform-app/src/app.rs:1862` |
| Fit to parameter range | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1161` |
| Fit to View | Show ▸ Fit to View (no document); Show ▸ Fit to View (model) | yes | `crates/keyform-app/src/app.rs:1249` |
| Fix Aspect Ratio | Home ▸ New Model… (no document) | yes | `crates/keyform-app/src/new_model_dialog.rs:56` |
| Flip horizontally | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:414` |
| Flip horizontally about x = | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:686` |
| Flip rotation deformer position | dialog via Modeling ▸ Edit Form ▸ Flip… (model); Flip dialog via Modeling ▸ Edit Form ▸ Flip… ▸ Parameters to reflect (model) | yes | `crates/keyform-app/src/form_dialogs.rs:753` |
| Flip sway horizontally | Modeling ▸ Auto Generation of Sway Motion… | by hand | `crates/keyform-app/src/generators_ui.rs:134` |
| Flip vertically | dialog via Modeling ▸ Edit Form ▸ Flip… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:415` |
| Flip vertically about y = | dialog via Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model) | yes | `crates/keyform-app/src/form_dialogs.rs:696` |
| Flip… | Modeling ▸ Edit Form ▸ Flip… (model); Modeling ▸ Edit Form ▸ Flip… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1656` |
| Fold line | the toolbar ▸ Path (model) | yes | `crates/keyform-app/src/path_tool.rs:788` |
| Follow Target | Animation ▸ Parameter Controller ▸ Follow Target (animation) | yes | `crates/keyform-app/src/animation.rs:1556` |
| Form Edit | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:1907` |
| Generate and Register Glue | Modeling ▸ Glue ▸ Generate and Register Glue (model); Modeling ▸ Glue ▸ Generate and Register Glue (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1457` |
| GIF | dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation); dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:364` |
| Glue | Modeling ▸ Glue (model); Modeling ▸ Glue (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1440` |
| Glue / bind the selected vertices | Mesh Edit mode ▸ Glue / bind the selected vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:1937` |
| Grid | Show ▸ Grid (no document); Show ▸ Grid (model) | yes | `crates/keyform-app/src/view_aids.rs:486` |
| Grid settings… | Show ▸ Grid ▸ Grid settings… (no document); Show ▸ Grid ▸ Grid settings… (model) | yes | `crates/keyform-app/src/view_aids.rs:492` |
| Group editing… | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:429` |
| Guide | Show ▸ Guide (no document); Show ▸ Guide (model) | yes | `crates/keyform-app/src/view_aids.rs:500` |
| Guide Settings (Modeling View)… | Show ▸ Guide ▸ Guide Settings (Modeling View)… (no document); Show ▸ Guide ▸ Guide Settings (Modeling View)… (model) | yes | `crates/keyform-app/src/view_aids.rs:537` |
| Help | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1359` |
| Home | Help ▸ Home (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1360` |
| Horizontal | Guide Settings dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… ▸ New Guide (model) | yes | `crates/keyform-app/src/view_aids.rs:403` |
| Idle Motion | Animation ▸ Idle Motion (no document); Animation ▸ Idle Motion (model) | yes | `crates/keyform-app/src/app.rs:2030` |
| Image… | File ▸ Export Image/Video ▸ Image… (no document); File ▸ Export Image/Video ▸ Image… (model) | yes | `crates/keyform-app/src/app.rs:997` |
| Import motion3.json… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1855` |
| Import Parameter Controller… | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:663` |
| Import Physics Settings… | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Physics (model) | yes | `crates/keyform-app/src/physics_editor.rs:268` |
| Include in motion export | dialog via Animation ▸ Fade ▸ Set fade-in/fade-out… (animation) | yes | `crates/keyform-app/src/animation.rs:1411` |
| Increase Output | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1143` |
| Increase priority | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1552` |
| Initialize Calculate FPS | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Physics (model) | yes | `crates/keyform-app/src/physics_editor.rs:283` |
| Input settings | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:174` |
| Insert key to every frame | dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation); dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation) ▸ Insert key to every frame | yes | `crates/keyform-app/src/animation.rs:1751` |
| Invert Mask | Apply Model Template dialog via Modeling ▸ Model Template ▸ Apply Template… ▸ FrontHair (model); Synthesize corners dialog via Modeling ▸ Parameter ▸ Synthesize corners… ▸ Face (model) | yes | `crates/keyform-app/src/palettes.rs:1275` |
| Keyed only | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:2048` |
| Language | Show ▸ Language (no document); dialog via File ▸ Environment Settings… (no document) | yes | `crates/keyform-app/src/app.rs:1236` |
| Lasso | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/app.rs:1957` |
| Light | Environment Settings dialog via File ▸ Environment Settings… ▸ Dark (no document) | yes | `crates/keyform-app/src/env_settings.rs:130` |
| Limit Settings for Blend Shape Weights… | Modeling ▸ Parameter ▸ Limit Settings for Blend Shape Weights… (model); Modeling ▸ Parameter ▸ Limit Settings for Blend Shape Weights… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1593` |
| Link animation file… | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:694` |
| Load Audio… | dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:836` |
| Lock | Modeling ▸ Lock (model); Create Warp Deformer dialog via Modeling ▸ Deformer ▸ Create Warp Deformer… ▸ Create (model) | yes | `crates/keyform-app/src/app.rs:1736` |
| Loop | Model Settings tab (model); Log tab (model) | yes | `crates/keyform-app/src/animation.rs:2029` |
| Loop motion | dialog via Animation ▸ Fade ▸ Set fade-in/fade-out… (animation) | yes | `crates/keyform-app/src/animation.rs:1402` |
| Make Background Transparent | dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation); dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:416` |
| Mapping Settings | Animation ▸ Motion-sync Settings…: tab (needs a motion-sync set; the audit's model has none) | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:448` |
| Mask | As above | by hand | `crates/keyform-app/src/template_dialog.rs:168` |
| Merge Vertices | Mesh Edit mode ▸ Merge Vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:1916` |
| Mesh edit | Modeling ▸ ArtMesh ▸ Mesh edit (model); Modeling ▸ ArtMesh ▸ Mesh edit (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1417` |
| Mesh Edit mode | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:1305` |
| Mesh reconstruction | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3862` |
| Mirror | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3866` |
| Mirror Edit | Mesh Edit mode ▸ Mirror Edit (mesh edit); Mesh Edit mode ▸ Mirror Edit ▸ Mirror Edit (mesh edit) | yes | `crates/keyform-app/src/app.rs:1920` |
| Model | File ▸ New ▸ Model (no document); File ▸ New ▸ Model (model) | yes | `crates/keyform-app/src/app.rs:889` |
| Model Template | Modeling ▸ Model Template (model); Modeling ▸ Model Template (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1697` |
| Model Template list | As above | by hand | `crates/keyform-app/src/template_dialog.rs:92` |
| Modeling | Window ▸ Workspace ▸ Modeling (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1309` |
| Motion Mirroring… | Modeling ▸ Parameter ▸ Motion Mirroring… (model); Modeling ▸ Parameter ▸ Motion Mirroring… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1532` |
| Motion-sync | Animation ▸ Motion-sync (no document); Animation ▸ Motion-sync (model) | yes | `crates/keyform-app/src/app.rs:2026` |
| Motion-sync Settings… | Modeling ▸ Parameter ▸ Motion-sync Settings… (model); Modeling ▸ Parameter ▸ Motion-sync Settings… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1603` |
| Move Here | Drag a row in the Parts palette and right-click the drop target (context menu of a drop target) | by hand | `crates/keyform-app/src/palettes.rs:568` |
| New | File ▸ New (no document); File ▸ New (model) | yes | `crates/keyform-app/src/app.rs:887` |
| New from current values | Model Settings tab (runtime model); Log tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:260` |
| New Guide | dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… (model); Guide Settings dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… ▸ New Guide (model) | yes | `crates/keyform-app/src/view_aids.rs:434` |
| New Model… | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:3186` |
| New Part | Modeling ▸ Part ▸ New Part (model); Modeling ▸ Part ▸ New Part (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1489` |
| New part | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:132` |
| New Scene | Animation ▸ Scene ▸ New Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1497` |
| No | dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… (model) | yes | `crates/keyform-app/src/app.rs:559` |
| No controller tool | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:598` |
| No thinning | dialog via File ▸ External Application Integration Settings… (no document); External Application Integration Settings dialog via File ▸ External Application Integration Settings… ▸ No thinning (no document) | yes | `crates/keyform-app/src/physics_editor.rs:674` |
| None | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ None (model) | yes | `crates/keyform-app/src/texture_atlas.rs:315` |
| Offscreen drawing | Inspector with a part selected | by hand | `crates/keyform-app/src/inspector.rs:456` |
| OK | dialog via Show ▸ Grid ▸ Grid settings… (no document); dialog via File ▸ Export Image/Video ▸ Image… (model) | yes | `crates/keyform-app/src/animation.rs:1454` |
| Open Recent File | File ▸ Open Recent File (no document); File ▸ Open Recent File (model) | yes | `crates/keyform-app/src/app.rs:918` |
| Open… | File ▸ Open… (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:910` |
| Options for Paste Form Special… | Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model); Modeling ▸ Edit Form ▸ Options for Paste Form Special… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1648` |
| Output All | dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation); dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:374` |
| Output Settings | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:175` |
| Output Work Area | dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation); dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:379` |
| Parameter | Window ▸ Parameter (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:1505` |
| Parameter Controller | Animation ▸ Parameter Controller (no document); Animation ▸ Parameter Controller (model) | yes | `crates/keyform-app/src/animation.rs:1555` |
| Parameter repeat | Animation ▸ Parameter repeat (no document); Animation ▸ Parameter repeat (model) | yes | `crates/keyform-app/src/app.rs:2036` |
| Part | Modeling ▸ Part (model); Modeling ▸ Part (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1487` |
| Part Settings… | Parts ▸ ☰ (mesh edit) | yes | `crates/keyform-app/src/palettes.rs:148` |
| Paste | Edit ▸ Paste (no document); Edit ▸ Paste (model) | yes | `crates/keyform-app/src/app.rs:1154` |
| Paste Form | Modeling ▸ Edit Form ▸ Paste Form (model); Modeling ▸ Edit Form ▸ Paste Form (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1628` |
| Paste output lines | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Edit (model) | yes | `crates/keyform-app/src/physics_editor.rs:261` |
| Paste Parameter Value | Edit ▸ Paste Parameter Value (no document); Edit ▸ Paste Parameter Value (model) | yes | `crates/keyform-app/src/app.rs:1186` |
| Physics | dialog via Modeling ▸ Physics Settings… (model) | yes | `crates/keyform-app/src/physics_editor.rs:266` |
| Physics Settings… | Modeling ▸ Physics Settings… (model); Modeling ▸ Physics Settings… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1820` |
| Play | dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… (model) | yes | `crates/keyform-app/src/motion_sync_dialog.rs:864` |
| Playback | Animation ▸ Playback Enter (no document); Animation ▸ Playback Enter (model) | yes | `crates/keyform-app/src/animation.rs:1482` |
| Position X | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Position X (model) | yes | `crates/keyform-app/src/physics_editor.rs:1058` |
| Preview | Modeling ▸ Edit Form ▸ Blend Form…: preview checkbox | by hand | `crates/keyform-app/src/form_dialogs.rs:617` |
| Prioritize duplicate names | As above: mapping options | by hand | `crates/keyform-app/src/template_dialog.rs:129` |
| Protect Edge exterior | main window (mesh edit); model.cmo3 tab (mesh edit) | yes | `crates/keyform-app/src/app.rs:3858` |
| Recent Files | File ▸ Recent Files (no document); File ▸ Recent Files (model) | yes | `crates/keyform-app/src/app.rs:925` |
| Record | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ▶ (model) | yes | `crates/keyform-app/src/physics_editor.rs:660` |
| Record motion and generate animation data | dialog via File ▸ External Application Integration Settings… (no document) | yes | `crates/keyform-app/src/external_host.rs:286` |
| Redisplay all dialogs that are hidden | Environment Settings dialog via File ▸ Environment Settings… ▸ Notification (no document) | yes | `crates/keyform-app/src/env_settings.rs:194` |
| Redo | Edit ▸ Redo (no document); Edit ▸ Redo (model) | yes | `crates/keyform-app/src/app.rs:1120` |
| Reflect | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Add… (model) | yes | `crates/keyform-app/src/physics_editor.rs:918` |
| Reflect Horizontally | Show ▸ Reflect Horizontally (no document); Show ▸ Reflect Horizontally (model) | yes | `crates/keyform-app/src/app.rs:1295` |
| Remove | File ▸ External Application Integration Settings…: per connected application; also Model Settings rows (motions, expressions) when present | by hand | `crates/keyform-app/src/external_host.rs:321` |
| Remove from model3.json | Model Settings (runtime model): next to each listed motion (session step: viewer motion adds one) | by hand | `crates/keyform-app/src/viewer_settings.rs:121` |
| Remove redundant keys | External Application Integration Settings dialog via File ▸ External Application Integration Settings… ▸ No thinning (no document); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ⚙ (model) | yes | `crates/keyform-app/src/physics_editor.rs:679` |
| Rename | Modeling ▸ Physics Settings…: group buttons ▸ Rename… shows the name field and this button | by hand | `crates/keyform-app/src/physics_editor.rs:1572` |
| Rename… | Motion-sync Settings dialog via Modeling ▸ Parameter ▸ Motion-sync Settings… ▸ Edit… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/animation.rs:1801` |
| Repeat | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ ▶ (model) | yes | `crates/keyform-app/src/physics_editor.rs:659` |
| Reset | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/palettes.rs:840` |
| Reset Axis Position | Mesh Edit mode ▸ Mirror Edit ▸ Reset Axis Position (mesh edit) | yes | `crates/keyform-app/src/app.rs:3750` |
| Reset Current Layout | Window ▸ Workspace ▸ Reset Current Layout (no document); Window ▸ Workspace ▸ Reset Current Layout (model) | yes | `crates/keyform-app/src/app.rs:1344` |
| Reset Dialog Size and Position | dialog via File ▸ Environment Settings… (no document); Environment Settings dialog via File ▸ Environment Settings… ▸ Reset Dialog Size and Position (no document) | yes | `crates/keyform-app/src/env_settings.rs:145` |
| Reset expression | Model Settings (runtime model) ▸ Expressions, with an expression selected | by hand | `crates/keyform-app/src/viewer_settings.rs:242` |
| Reset Magnification | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1169` |
| Reset Mesh | Mesh Edit mode ▸ Reset Mesh (mesh edit) | yes | `crates/keyform-app/src/app.rs:1894` |
| Reset Output Maximum | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Output Settings (model) | yes | `crates/keyform-app/src/physics_editor.rs:1175` |
| Reset parameters | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/app.rs:2240` |
| Reset Rotation | Show ▸ Reset Rotation (no document); Show ▸ Reset Rotation (model) | yes | `crates/keyform-app/src/app.rs:1281` |
| Reset to default values | Modeling ▸ Parameter ▸ Reset to default values (model); Modeling ▸ Parameter ▸ Reset to default values (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1509` |
| Restriction for SDK (handles move vertically only) | dialog via Animation ▸ Fade ▸ Set fade-in/fade-out… (animation) | yes | `crates/keyform-app/src/animation.rs:1405` |
| Reverse Clipping ID Lookup | Modeling ▸ Clipping ID ▸ Reverse Clipping ID Lookup (model); Modeling ▸ Clipping ID ▸ Reverse Clipping ID Lookup (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1685` |
| Run Auto Layout | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:252` |
| Save as… | File ▸ Save as… (no document); File ▸ Save as… (model) | yes | `crates/keyform-app/src/app.rs:983` |
| Save model3.json | File ▸ Save model3.json (runtime model); Model Settings tab (runtime model) | yes | `crates/keyform-app/src/viewer_settings.rs:48` |
| Scene | Animation ▸ Scene (no document); Animation ▸ Scene (model) | yes | `crates/keyform-app/src/animation.rs:1496` |
| scene | dialog via File ▸ Export Image/Video ▸ Video… (animation); dialog via File ▸ Export Image/Video ▸ Video… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:408` |
| Scene settings… | Deformer ▸ Scene (animation) | yes | `crates/keyform-app/src/animation.rs:1793` |
| Select all | dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation); dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation) ▸ Insert key to every frame | yes | `crates/keyform-app/src/animation.rs:1730` |
| Select All | Edit ▸ Select All (no document); Edit ▸ Select All (model) | yes | `crates/keyform-app/src/app.rs:1200` |
| Select Clipping Mask | Modeling ▸ Clipping ID ▸ Select Clipping Mask (model); Modeling ▸ Clipping ID ▸ Select Clipping Mask (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1672` |
| Select Model Template… | Modeling ▸ Model Template ▸ Apply Model Template…, after choosing a template file | by hand | `crates/keyform-app/src/template_dialog.rs:86` |
| Select/Edit Controller | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:583` |
| Selected frame | dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation); dialog via File ▸ Export Image/Video ▸ GIF Animation… (animation) ▸ Output Work Area | yes | `crates/keyform-app/src/export_media.rs:384` |
| Selected objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:844` |
| Set | Animation ▸ Motion-sync Settings…: mapping row | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:605` |
| Set fade-in/fade-out… | Animation ▸ Fade ▸ Set fade-in/fade-out… (animation) | yes | `crates/keyform-app/src/animation.rs:1545` |
| Set Random Pose | Modeling ▸ Set Random Pose (model); Modeling ▸ Set Random Pose (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1830` |
| Setting | dialog via Show ▸ Guide ▸ Guide Settings (Modeling View)… (model) | yes | `crates/keyform-app/src/view_aids.rs:337` |
| Show | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1220` |
| Show Grid (Modeling View) | Show ▸ Grid ▸ Show Grid (Modeling View) (no document); Show ▸ Grid ▸ Show Grid (Modeling View) (model) | yes | `crates/keyform-app/src/view_aids.rs:487` |
| Show Guide (Modeling View) | Show ▸ Guide ▸ Show Guide (Modeling View) (no document); Show ▸ Guide ▸ Show Guide (Modeling View) (model) | yes | `crates/keyform-app/src/view_aids.rs:501` |
| Show parameter ID and effectiveness in tooltip | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1481` |
| Show parameter name and effectiveness in tooltip | Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Group editing… (model) | yes | `crates/keyform-app/src/physics_editor.rs:1480` |
| Show recent files with full paths | Environment Settings dialog via File ▸ Environment Settings… ▸ File (no document) | yes | `crates/keyform-app/src/env_settings.rs:153` |
| Show/Hide | Modeling ▸ Show/Hide (model); Modeling ▸ Show/Hide (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1764` |
| Skip invisible objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:852` |
| Skip locked objects | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:851` |
| Snap Moving Vertices to Guide | Show ▸ Guide ▸ Snap Moving Vertices to Guide (no document); Show ▸ Guide ▸ Snap Moving Vertices to Guide (model) | yes | `crates/keyform-app/src/view_aids.rs:546` |
| Still images (JPEG) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ GIF Animation… ▸ GIF (animation); Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ Still Image (Sequence)… ▸ Still images (PNG) (animation) | yes | `crates/keyform-app/src/export_media.rs:359` |
| Still images (PNG) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ GIF Animation… ▸ GIF (animation); dialog via File ▸ Export Image/Video ▸ Still Image (Sequence)… (animation) | yes | `crates/keyform-app/src/export_media.rs:354` |
| Stop | Animation ▸ Motion-sync Settings…: audio preview playing | by hand | `crates/keyform-app/src/motion_sync_dialog.rs:860` |
| Subtools | Mesh Edit mode ▸ Subtools (mesh edit) | yes | `crates/keyform-app/src/app.rs:1956` |
| Synchronize left and right side setting values | Modeling ▸ face generator dialog (Auto Generation of Face Motion…) | by hand | `crates/keyform-app/src/generators_ui.rs:138` |
| Synthesize corners… | Modeling ▸ Parameter ▸ Synthesize corners… (model); Modeling ▸ Parameter ▸ Synthesize corners… (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1521` |
| Target follow | Deformer ▸ Controllers (animation) | yes | `crates/keyform-app/src/param_ctrl_ui.rs:600` |
| Texture Atlas | Modeling ▸ Texture Atlas (model); dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… (model) | yes | `crates/keyform-app/src/app.rs:1720` |
| Top left | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:820` |
| Top right | dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model); dialog via Modeling ▸ Parameter ▸ Synthesize corners… (model) ▸ Default value | yes | `crates/keyform-app/src/form_dialogs.rs:821` |
| Track | Animation ▸ Track (no document); Animation ▸ Track (model) | yes | `crates/keyform-app/src/animation.rs:1513` |
| Track cursor | Animation ▸ Track cursor (no document); Animation ▸ Track cursor (model) | yes | `crates/keyform-app/src/app.rs:2031` |
| Unbind | As Bind | by hand | `crates/keyform-app/src/app.rs:3841` |
| Unbind Glue Vertices | Modeling ▸ Glue ▸ Unbind Glue Vertices (model); Modeling ▸ Glue ▸ Unbind Glue Vertices (model, mesh selected) | yes | `crates/keyform-app/src/app.rs:1473` |
| Undo | Edit ▸ Undo (no document); Edit ▸ Undo (model) | yes | `crates/keyform-app/src/app.rs:1110` |
| Unglue / unbind the selected vertices | Mesh Edit mode ▸ Unglue / unbind the selected vertices (mesh edit) | yes | `crates/keyform-app/src/app.rs:1947` |
| Unlink | Modeling ▸ Physics Settings…: linked-animation menu, after "Link animation file…" linked a file | by hand | `crates/keyform-app/src/physics_editor.rs:703` |
| Unselect all | dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation); dialog via Animation ▸ Track ▸ Bake Animation from Physics… (animation) ▸ Insert key to every frame | yes | `crates/keyform-app/src/animation.rs:1733` |
| Up | dialog via Modeling ▸ Physics Settings… (model); Physics Settings dialog via Modeling ▸ Physics Settings… ▸ Up (model) | yes | `crates/keyform-app/src/physics_editor.rs:1078` |
| Update keyform | Generator dialogs (sway / 3D rotation / face): apply button | by hand | `crates/keyform-app/src/generators_ui.rs:151` |
| Use Center of Canvas | Modeling ▸ Edit Form ▸ Motion Mirroring…: mirror axis choice (disabled when only deformers are selected) | by hand | `crates/keyform-app/src/form_dialogs.rs:418` |
| Vertical | Show ▸ Guide ▸ Guide Settings (Modeling View)…: the Type combo of a guide row (after New Guide or Add Guide to Center) | by hand | `crates/keyform-app/src/view_aids.rs:408` |
| Video (MP4) | Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ GIF Animation… ▸ GIF (animation); Image/Video Output Settings dialog via File ▸ Export Image/Video ▸ Still Image (Sequence)… ▸ Still images (PNG) (animation) | yes | `crates/keyform-app/src/export_media.rs:365` |
| Viewer | Window ▸ Workspace ▸ Viewer (no document); main window (no document) | yes | `crates/keyform-app/src/app.rs:2093` |
| Visible model images | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Add texture… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:305` |
| Whole model images (not only the mesh bounds) | Texture Atlas dialog via Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… ▸ Auto Layout… (model) | yes | `crates/keyform-app/src/texture_atlas.rs:228` |
| Window | main window (no document); Parameter tab (no document) | yes | `crates/keyform-app/src/app.rs:1312` |
| Workspace | Window ▸ Workspace (no document); Window ▸ Workspace (model) | yes | `crates/keyform-app/src/app.rs:1320` |
| Yes | Closing a modified document (File ▸ Close): save prompt | by hand | `crates/keyform-app/src/app.rs:556` |
| Zoom In | Show ▸ Zoom In (no document); Show ▸ Zoom In (model) | yes | `crates/keyform-app/src/app.rs:1259` |
| Zoom Out | Show ▸ Zoom Out (no document); Show ▸ Zoom Out (model) | yes | `crates/keyform-app/src/app.rs:1271` |
| — | dialog via Modeling ▸ Parameter ▸ Auto Generation of Face Motion ▸ Generate Face Deformer… (model); Auto Generation of Face Motion dialog via Modeling ▸ Parameter ▸ Auto Generation of Face Motion ▸ Generate Face Deformer… ▸ — (model) | yes | `crates/keyform-app/src/template_dialog.rs:146` |
| ⏹ | main window (animation); model.can3 tab (animation) | yes | `crates/keyform-app/src/animation.rs:2022` |
| ☰ | main window (model); model.cmo3 tab (model) | yes | `crates/keyform-app/src/palettes.rs:130` |
| ✕ | Animation workspace ▸ Events menu: delete button next to each event (after "Add at frame …") | by hand | `crates/keyform-app/src/animation.rs:1956` |

## Shortcuts

| Feature | How to reach it | Checked | Code |
|---|---|---|---|
| A | shown in the tooltip of Arrow | yes | `crates/keyform-app/src/app.rs:2853` |
| Backspace | Same as Delete (Edit ▸ Delete shows "Delete"); for keyboards without a Delete key | by hand | `crates/keyform-app/src/app.rs:2834` |
| Ctrl+1 | shown on Modeling ▸ Parameter ▸ Reset to default values | yes | `crates/keyform-app/src/app.rs:2795` |
| Ctrl+2 | shown on Modeling ▸ Lock ▸ Lock/unlock Selected Objects | yes | `crates/keyform-app/src/app.rs:2772` |
| Ctrl+3 | shown on Modeling ▸ Show/Hide ▸ Show/Hide Selected Objects | yes | `crates/keyform-app/src/app.rs:2784` |
| Ctrl+4 | shown on Modeling ▸ Parameter ▸ Synthesize corners… | yes | `crates/keyform-app/src/app.rs:2902` |
| Ctrl+A | shown on Edit ▸ Select All | yes | `crates/keyform-app/src/app.rs:2988` |
| Ctrl+E | shown on Modeling ▸ ArtMesh ▸ Mesh edit | yes | `crates/keyform-app/src/app.rs:2964` |
| Ctrl+G | shown on Modeling ▸ Glue ▸ Generate and Register Glue | yes | `crates/keyform-app/src/app.rs:2844` |
| Ctrl+I | shown on File ▸ Export Image/Video ▸ Still Image (Sequence)… | yes | `crates/keyform-app/src/app.rs:2748` |
| Ctrl+N | shown on File ▸ New ▸ Model | yes | `crates/keyform-app/src/app.rs:2936` |
| Ctrl+R | shown on Mesh Edit mode ▸ Auto attach Edge | yes | `crates/keyform-app/src/app.rs:2954` |
| Ctrl+S | shown on File ▸ Save | yes | `crates/keyform-app/src/app.rs:2729` |
| Ctrl+Shift+2 | shown on Modeling ▸ Lock ▸ Lock the unselected objects | yes | `crates/keyform-app/src/app.rs:2776` |
| Ctrl+Shift+3 | shown on Modeling ▸ Show/Hide ▸ Hide the unselected objects | yes | `crates/keyform-app/src/app.rs:2788` |
| Ctrl+Shift+A | shown on Modeling ▸ ArtMesh ▸ Automatic Mesh generator… | yes | `crates/keyform-app/src/app.rs:2948` |
| Ctrl+Shift+B | shown on Modeling ▸ Edit Form ▸ Blend Form… | yes | `crates/keyform-app/src/app.rs:2901` |
| Ctrl+Shift+C | shown on Modeling ▸ Edit Form ▸ Copy Form | yes | `crates/keyform-app/src/app.rs:2899` |
| Ctrl+Shift+G | shown on Modeling ▸ Glue ▸ Unbind Glue Vertices | yes | `crates/keyform-app/src/app.rs:2842` |
| Ctrl+Shift+M | shown on File ▸ Export Image/Video ▸ Video… | yes | `crates/keyform-app/src/app.rs:2747` |
| Ctrl+Shift+O | shown on File ▸ Open Recent File | yes | `crates/keyform-app/src/app.rs:2739` |
| Ctrl+Shift+S | shown on File ▸ Save as… | yes | `crates/keyform-app/src/app.rs:2728` |
| Ctrl+Shift+V | shown on Modeling ▸ Edit Form ▸ Paste Form | yes | `crates/keyform-app/src/app.rs:2900` |
| Ctrl+Shift+Z | Second Redo shortcut (macOS order, menus.md); Edit ▸ Redo shows Ctrl+Y | by hand | `crates/keyform-app/src/app.rs:3006` |
| Ctrl+T | shown on Modeling ▸ Texture Atlas ▸ Edit Texture Atlas… | yes | `crates/keyform-app/src/app.rs:2942` |
| Ctrl+Y | shown on Edit ▸ Redo | yes | `crates/keyform-app/src/app.rs:3005` |
| Ctrl+Z | shown on Edit ▸ Undo | yes | `crates/keyform-app/src/app.rs:3007` |
| Delete | shown on Edit ▸ Delete | yes | `crates/keyform-app/src/app.rs:2834` |
| Escape | shown on Edit ▸ Deselect | yes | `crates/keyform-app/src/app.rs:2989` |
| L | shown in the tooltip of Lasso | yes | `crates/keyform-app/src/app.rs:2854` |
| P | shown in the tooltip of Path | yes | `crates/keyform-app/src/app.rs:2856` |

## Menu items never enabled

| Feature | How to reach it | Checked | Code |
|---|---|---|---|
| Animation ▸ Track ▸ Bake Animation from Motion-sync | Enabled in an animation whose scene has a sound track and whose model has motion-sync settings (Modeling ▸ Parameter ▸ Motion-sync Settings…); like the timeline Scene menu entry | by hand | |
| Edit ▸ Paste | Enabled after Edit ▸ Copy or Cut (Ctrl+C / Ctrl+X) of an art mesh or deformer in the Modeling workspace | by hand | |
| Edit ▸ Paste Parameter Value | Enabled after Edit ▸ Copy Parameter Value | by hand | |
| Edit ▸ Redo | Enabled after an Undo (Edit ▸ Undo, Ctrl+Z); the audit's states have no undo history | by hand | |
| Mesh Edit mode ▸ Delete Vertex | In mesh edit with vertices selected (click or rubber-band vertices first) | by hand | |
| Mesh Edit mode ▸ Mirror Edit ▸ Center of Selected Vertices | In mesh edit with vertices selected | by hand | |
| Modeling ▸ Edit Form ▸ Blend Form… | Enabled after Edit Form ▸ Copy Form (Ctrl+Shift+C), like Paste Form | by hand | |
| Modeling ▸ Edit Form ▸ Paste Form | Enabled after Edit Form ▸ Copy Form (Ctrl+Shift+C) with a mesh or deformer selected | by hand | |
| Show ▸ Guide ▸ Add Guides at Coordinates of Selected Vertices | Enabled with vertices selected (Arrow tool: press on a vertex of a selected art mesh) | by hand | |

## Spec menu coverage

Every spec menu item (`keyform-spec/50-ui/menus.md`) that isn't in Keyform's menus at its
spec place, sorted by `tools/e2e/menu_coverage.py` (label matching against what the
audit saw and the app's strings, then hand review in
`tools/e2e/menu-coverage-overrides.tsv`).

| Class | Meaning | Count |
|---|---|---|
| a | excluded by policy | 8 |
| p | in a Keyform menu under another name or place (rename / move only) | 1 |
| b | present elsewhere — needs its menu entry | 16 |
| b? | probably present elsewhere (fuzzy match, to review) | 0 |
| c | missing feature | 203 |
| k+ | keyboard-only command, present | 56 |
| k- | keyboard-only command, missing | 106 |

### File

| Spec item | Class | Where it is now / note |
|---|---|---|
| Export Image/Video ▸ PSD images(beta) [Ctrl+P] | c |  |
| Keyboard Shortcuts [Ctrl+Alt+Shift+K] | c |  |
| Model Statistics | c |  |
| Canvas Settings | c |  |
| Resize Model | c |  |

### Edit

| Spec item | Class | Where it is now / note |
|---|---|---|
| Deselect Hidden and Locked Objects | c |  |
| Search and Replace [Ctrl+F] | c |  |

### Show

| Spec item | Class | Where it is now / note |
|---|---|---|
| Guide ▸ Show Guide (Animation view) | c |  |
| Show ArtMesh vertex | c |  |
| Show ArtMesh's Vertex Number | c |  |
| Highlight vertices outside parent deformer | c |  |
| Highlight Deformer's Child Elements [Ctrl+Shift+D] | c |  |
| Highlight overlapping polygons | c |  |
| Show Child Elements When Resize Warp Deformer | c |  |
| Show Object Name at Cursor Position | c |  |
| Hide selected state | c |  |
| Focus on Selected Object [F] | c |  |
| Model Image | c |  |
| Texture Atlas | c | display type: show the texture atlas instead of the model image (not the atlas editor, which is Modeling ▸ Texture Atlas) |
| Display Quality | c |  |
| Display Quality ▸ Full Scale | c |  |
| Display Quality ▸ 1/2 | c |  |
| Display Quality ▸ 1/4 | c |  |
| Display Quality ▸ 1/8 | c |  |
| Show/Hide Bounding Box | c |  |
| Onion skin | c |  |
| Onion skin ▸ Modeling View | c | onion skin not implemented (view-aids.md §4) |
| Onion skin ▸ Animation View | c |  |
| Onion skin ▸ Form Animation View | c |  |
| Onion skin ▸ Onion skin setting | c |  |
| Track Trajectory | c |  |
| Track Trajectory ▸ Animation View | c |  |
| Track Trajectory ▸ Track Trajectory Settings | c |  |

### Modeling

| Spec item | Class | Where it is now / note |
|---|---|---|
| Deformer ▸ Auto Generation of Deformer | c |  |
| Deformer ▸ Select Deformer for Parent [Ctrl+D] | c |  |
| Deformer ▸ Select Deformer's Child Elements [Ctrl+Alt+D] | c |  |
| Deformer ▸ Delete Deformer and its Content [Ctrl+Delete] | c |  |
| Deformer ▸ Apply Deformer to Child Elements | c |  |
| Deformer ▸ Validate Deformer | c |  |
| Deformer ▸ Align Angle of Bounding Box | c |  |
| Parameter ▸ Lock Default Form [Ctrl+Shift+1] | c |  |
| Parameter ▸ Synchronize Parameter Values Across Tabs | c |  |
| Parameter ▸ Parameter settings | c |  |
| Parameter ▸ Parameter Group Settings | c |  |
| Parameter ▸ Set All Parameter Values as Default | c |  |
| Parameter ▸ Multiple Keys Editing | c |  |
| Parameter ▸ Select Object Linked to Key | c |  |
| Parameter ▸ Arrange Parameter Configuration for Selected Object | c |  |
| Parameter ▸ Settings for Eye Blinking and Lip-sync | c |  |
| Parameter ▸ Validate Assigned Parameters | c |  |
| ArtMesh ▸ Change Draw Order of All ArtMeshes | c |  |
| ArtMesh ▸ Bulk Change Draw Order of ArtMeshes | c |  |
| ArtMesh ▸ Create artMesh for collision detection | c |  |
| ArtPath | b | toolbar ArtPath tool; Tool Details has the tool settings |
| ArtPath ▸ Automatic generation from a single ArtMesh | c |  |
| ArtPath ▸ Automatic generation from multiple ArtMeshes | c |  |
| ArtPath ▸ Apply Color | c |  |
| ArtPath ▸ Apply Opacity | c |  |
| ArtPath ▸ Apply Line Width | c |  |
| ArtPath ▸ Apply All | c |  |
| ArtPath ▸ Align with Outline ArtMesh | c |  |
| ArtPath ▸ Align with Fill ArtMesh | c |  |
| ArtPath ▸ Add control points to align with ArtMesh | c |  |
| ArtPath ▸ Add a point in the middle of all control points | c |  |
| ArtPath ▸ Add a point in the middle of the selected control point | c |  |
| ArtPath ▸ Open Auto Generation ArtPath Dialog | c |  |
| ArtPath ▸ Create DeformPath from ArtPath | c |  |
| ArtPath ▸ Bake ArtMesh deformation to ArtPath | c |  |
| ArtPath ▸ Follow all | c |  |
| ArtPath ▸ Cancel all | c |  |
| ArtPath ▸ Follow selected control points | c |  |
| ArtPath ▸ Release selected control point | c |  |
| ArtPath ▸ Open ArtPath Brush Settings Dialog | c |  |
| Glue ▸ Glue On/Off | c |  |
| Skinning | c |  |
| Skinning ▸ Skinning | c |  |
| Skinning ▸ Use deformation path for Skinning | c |  |
| Skinning ▸ Generate parameters for rotation deformer | c |  |
| Edit Form ▸ Confirm target when pasting multiple forms | c |  |
| Edit Form ▸ Zoom In / Zoom Out Specifying Magnification | c |  |
| Edit Form ▸ Shape Warp Deformer along Bezier Handle | c |  |
| Edit Form ▸ Expand Warp Deformer Based on Children Outside Range | c |  |
| Edit Form ▸ Revert to Original | c |  |
| Edit Form ▸ Update Original Shape of Deformers | c |  |
| Edit Form ▸ Delete Original Shape of All Deformers | c |  |
| Temporary deform tool | c |  |
| Temporary deform tool ▸ Enlarge/shrink | c |  |
| Temporary deform tool ▸ Rotation | c |  |
| Temporary deform tool ▸ Free transform | c |  |
| Temporary deform tool ▸ Perspective | c |  |
| Temporary deform tool ▸ Distort | c |  |
| Temporary deform tool ▸ Temporary warp deformation | c |  |
| Temporary deform tool ▸ Add columns | c |  |
| Temporary deform tool ▸ Remove columns | c |  |
| Temporary deform tool ▸ Add rows | c |  |
| Temporary deform tool ▸ Remove rows | c |  |
| Temporary deform tool ▸ Temporary path deformation | c |  |
| Temporary deform tool ▸ Deselect the temporary deform tool | c |  |
| Texture Atlas ▸ Switch between Model Image and Texture Atlas [T] | c |  |
| Texture Atlas ▸ Apply Texture Atlas to Model Image | c |  |
| Show/Hide ▸ Show/Hide Guide Image [Ctrl+Alt+Shift+3] | c |  |
| Show/Hide ▸ Restore to Previous Visibility State | c |  |
| Convert Model ID | c |  |
| Bulk setting of model objects | c |  |
| Bulk setting of model objects ▸ Export | c |  |
| Bulk setting of model objects ▸ Import | c |  |
| Bulk setting of model parameters | c |  |
| Bulk setting of model parameters ▸ Export | c |  |
| Bulk setting of model parameters ▸ Import | c |  |

### Animation

| Spec item | Class | Where it is now / note |
|---|---|---|
| Material | c |  |
| Material ▸ Reload Material of Selected Track | c |  |
| Material ▸ Replace Material of Selected Track | c |  |
| Material ▸ Replace Material of Selected Track | c |  |
| Scene ▸ Import Scene | c |  |
| Scene ▸ Generate Scenes Automatically from Audio Files | c |  |
| Scene ▸ Enable/Disable Loop Editing Support | c |  |
| Track ▸ Apply Lip-sync from the Audio File | c |  |
| Track ▸ Start Track at Current Position [Shift+B] | c |  |
| Track ▸ End Track at Current Position [Shift+N] | c |  |
| Track ▸ Move Track to Current Position [Shift+M] | c |  |
| Timeline | c |  |
| Timeline ▸ Move to Beginning [Home] | c |  |
| Timeline ▸ Move to End [End] | c |  |
| Timeline ▸ Move to Start Frame of Work Area [Shift+Home] | c |  |
| Timeline ▸ Move to End Frame of Work Area [Shift+End] | c |  |
| Timeline ▸ Previous frame [Page Up] | c |  |
| Timeline ▸ Next frame [Page Down] | c |  |
| Timeline ▸ Previous Keyframe [J] | c |  |
| Timeline ▸ Next Keyframe [K] | c |  |
| Timeline ▸ Previous Keyframe in Track [Shift+J] | c |  |
| Timeline ▸ Next Keyframe in Track [Shift+K] | c |  |
| Timeline ▸ Return to Previous Marker [Ctrl+J] | c |  |
| Timeline ▸ Proceed to Next Marker [Ctrl+K] | c |  |
| Timeline ▸ Return to the previous onion skin marker [Ctrl+Shift+J] | c |  |
| Timeline ▸ Go to the next onion skin marker [Ctrl+Shift+K] | c |  |
| Timeline ▸ Set as Start Frame of Work Area | c |  |
| Timeline ▸ Set as End Frame of Work Area | c |  |
| Timeline ▸ Display Entire Work Area [Shift+F] | c |  |
| Timeline ▸ Display Entire Scene | c |  |
| Timeline ▸ Reset Zoom | c |  |
| Fade ▸ Apply Fade-In/Fade-Out Settings to Other Scenes | c |  |
| Parameter Controller ▸ Parameter Controller Calculation Unit Settings | c |  |
| Parameter Controller ▸ Reverse Direction Controller | c |  |
| Parameter Controller ▸ Convert Direction Controller into 2 Single Point Controllers | c |  |
| Parameter Controller ▸ Convert 2 Single Point Controllers into Direction Controller | c |  |
| Parameter Controller ▸ Calculate collectively before playback | c |  |
| Parameter Controller ▸ Show Controller | c |  |
| Parameter Controller ▸ Controller Display Settings | c |  |
| Parameter Bookmarks | c |  |
| Parameter Bookmarks ▸ Display with Selected Parameters | c |  |
| Parameter Bookmarks ▸ Delete Labels from Selected Tracks | c |  |
| Parameter Bookmarks ▸ Duplicate All Labels | c |  |
| Parameter Bookmarks ▸ Duplicate Labels Individually | c |  |
| Parameter Bookmarks ▸ Hide All Labels | c |  |
| Parameter Bookmarks ▸ Parameter Bookmark Display Settings | c |  |
| Add Marker | c |  |
| Switch Edit Mode [C] | c |  |
| Gather files | c |  |

### Form Animation

| Spec item | Class | Where it is now / note |
|---|---|---|
| Deformer | p | Modeling ▸ Deformer |
| Deformer ▸ Select Deformer for Parent [Ctrl+D] | c |  |
| Deformer ▸ Select Deformer's Child Elements [Ctrl+Alt+D] | c |  |
| Temporary deform tool | c |  |
| Temporary deform tool ▸ Enlarge/shrink | c |  |
| Temporary deform tool ▸ Rotation | c |  |
| Temporary deform tool ▸ Free transform | c |  |
| Temporary deform tool ▸ Perspective | c |  |
| Temporary deform tool ▸ Distort | c |  |
| Temporary deform tool ▸ Temporary warp deformation | c |  |
| Temporary deform tool ▸ Add columns | c |  |
| Temporary deform tool ▸ Remove columns | c |  |
| Temporary deform tool ▸ Add rows | c |  |
| Temporary deform tool ▸ Remove rows | c |  |
| Temporary deform tool ▸ Temporary path deformation | c |  |
| Temporary deform tool ▸ Deselect the temporary deform tool | c |  |
| Clipping ID | b | Modeling ▸ Clipping ID (model) |
| Clipping ID ▸ Select Clipping Mask | b | Inspector ▸ Add mask… |
| Clipping ID ▸ Reverse Clipping ID Lookup | b | Parts palette: right-click a mask mesh row |
| Lock | b | Parts palette lock icons and ☰ menu |
| Lock ▸ Lock/unlock Selected Objects [Ctrl+2] | b | Modeling ▸ Lock ▸ Lock/unlock Selected Objects (model) |
| Lock ▸ Lock the unselected objects [Ctrl+Shift+2] | b | Modeling ▸ Lock ▸ Lock the unselected objects (model) |
| Lock ▸ Unlock all [Ctrl+Alt+2] | b | Parts palette ☰ menu |
| Show/Hide | b | Modeling ▸ Show/Hide (model) |
| Show/Hide ▸ Show/Hide Selected Objects [Ctrl+3] | b | Modeling ▸ Show/Hide ▸ Show/Hide Selected Objects (model) |
| Show/Hide ▸ Hide the unselected objects [Ctrl+Shift+3] | b | Modeling ▸ Show/Hide ▸ Hide the unselected objects (model) |
| Show/Hide ▸ Show all [Ctrl+Alt+3] | b | Parts palette ☰ menu |
| Show/Hide ▸ Restore to Previous Visibility State | c |  |
| Edit Level | b | toolbar Edit Level 1/2/3 |
| Edit Level ▸ Level 1 [1] | b | toolbar button 1 and key 1 |
| Edit Level ▸ Level 2 [2] | b | toolbar button 2 and key 2 |
| Edit Level ▸ Level 3 [3] | b | toolbar button 3 and key 3 |
| Create form edit key | c |  |
| Animation preview setting | c |  |

### Mesh Edit mode

| Spec item | Class | Where it is now / note |
|---|---|---|
| Paste with Copied Shape [Ctrl+Alt+V] | c |  |
| Flip horizontally | c | the Edit Form ▸ Flip… dialog flips forms, not mesh-edit vertices |
| Flip vertically | c | as Flip horizontally |
| Invert Axis | c |  |
| Stroke mesh-mapping | c |  |
| Stroke mesh-mapping ▸ Increase mesh width | c |  |
| Stroke mesh-mapping ▸ Reduce mesh width | c |  |
| Stroke mesh-mapping ▸ Increase divisions | c |  |
| Stroke mesh-mapping ▸ Reduce divisions | c |  |
| Stroke mesh-mapping ▸ Change the number of vertices in the mesh width | c |  |
| Stroke mesh-mapping ▸ Confirm stroke [Shift+E] | c |  |
| Subtools ▸ Delete Vertex | c | no Delete Vertex subtool (Delete Vertex is a command in the Mesh Edit mode menu) |
| Subtools ▸ Add points | c | no "Add points" subtool yet |
| Subtools ▸ Stroke mesh-mapping | c |  |

### Window

| Spec item | Class | Where it is now / note |
|---|---|---|
| Workspace ▸  [CMenuItem] | a | spec placeholder for the workspace list; Keyform lists Modeling / Viewer / Animation there |
| Workspace ▸ Update Initial Layout | c |  |
| Workspace ▸ Workspace Settings | c |  |
| Hide Palette | c |  |
| Show Palette | c |  |
| Duplicate Tab | c |  |
| Switch to Previous Tab | c |  |
| Switch to Next Tab | c |  |
| Set All Views to Same Display Position | c |  |
| Reset Status | c |  |

### Help

| Spec item | Class | Where it is now / note |
|---|---|---|
| User Manual | c | should open Keyform's documentation |
| Go to Live2D Web Page | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |
| Product Page | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |
| Download Page | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |
| Sample Model | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |
| Creators Forum | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |
| Live2D Help | c | should open Keyform's documentation |
| Tutorial Video | a |  |
| Display Operation | c |  |
| Open Log File | c |  |
| Open Auto Backup Folder | c |  |
| Update History | a | Live2D web pages / products (spec: Help links point to Keyform's own pages instead) |

### Keyboard-only commands

| Spec item | Class | Where it is now / note |
|---|---|---|
| Rotation Deformer Creation Tool | k- |  |
| Arrow tool [A] | k+ | main window (model) |
| Lasso Tool [L] | k+ | Image Output Settings dialog via File ▸ Export Image/Video ▸ Image… ▸ Lasso (model) |
| Brush Selection tool [B] | k+ | similar: tooltip of Soft in the toolbar (model) |
| Deform Path edit [P] | k- |  |
| Deform Brush Tool | k- |  |
| Edit Glue | k- |  |
| ArtPath Tools | k+ | similar: tooltip of ArtPath in the toolbar (model) |
| Parameter Controller | k+ | Animation ▸ Parameter Controller (no document) |
| Switch to Last Used Tool | k- |  |
| Switch Workspace to Modeling | k- |  |
| Switch Workspace to Animation | k- |  |
| Switch workspace to Form Animation | k- |  |
| Lock ArtMesh [Shift+A] | k- |  |
| Lock Deformer [Shift+D] | k- |  |
| Show/hide ArtMesh | k- |  |
| Solo [S] | k- |  |
| Apply Opacity | k- |  |
| Apply multiply color/screen color | k- |  |
| Select Objects of Selected Vertices | k- |  |
| Snapshot [Ctrl+5] | k- |  |
| Show/hide snapshot [5] | k- |  |
| Save snapshot guide image [Ctrl+Shift+5] | k- |  |
| Zoom out | k+ | Show ▸ Zoom Out (no document) |
| Zoom in | k+ | Show ▸ Zoom In (no document) |
| Display as original size | k- |  |
| Display All [Ctrl+0] | k- |  |
| Flip canvas [R] | k- |  |
| Play/Stop Random Pose | k- |  |
| Set Random Pose | k+ | Modeling ▸ Set Random Pose (model) |
| Lightweight playback | k- |  |
| Highlight Frame Step frames | k- |  |
| Auto-scroll to Form Animation property of selected object | k- |  |
| Automatically scroll to properties of current parameter controller | k- |  |
| Enable moving indicator out of scene range | k- |  |
| Enable Operation of Keyframes Hidden by Shy | k- |  |
| Expand all tracks | k- |  |
| Select all keys when selecting parameters | k- |  |
| Exclude Frame Step When Bulk Inserting Keyframes | k- |  |
| Display 1 Frame Range at Indicator | k- |  |
| Display key values | k- |  |
| Display key name | k- |  |
| Auto resize | k- |  |
| Always edit continuous continuous Beziers | k- |  |
| Smart snap using Shift | k- |  |
| Make Beziers by dragging curves | k- |  |
| Snap to key position on curve | k- |  |
| Freeze/Unfreeze at Current Frame | k- |  |
| Freeze All | k- |  |
| Freeze Selected Properties | k- |  |
| Freeze Unselected Properties | k- |  |
| Unfreeze All | k- |  |
| Make art path control points thicker | k- |  |
| Make art path control points thinner | k- |  |
| Snap to Nearest Key | k- |  |
| Lock Draw Order Slider | k- |  |
| Tool Details | k+ | Window ▸ Tool Details (no document) |
| Tool Details ▸ Deform Path edit | k- |  |
| Tool Details ▸ Deform Path edit ▸ Add control point | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ Deform Path edit ▸ Delete control point | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ Deform Path edit ▸ Delete path | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ Deform Path edit ▸ Fold line | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ Edit Glue | k- |  |
| Tool Details ▸ Edit Glue ▸ Weight brush A:B | k+ | Modeling ▸ Glue ▸ Weight brush A:B (model) |
| Tool Details ▸ Edit Glue ▸ Weight brush A | k+ | Modeling ▸ Glue ▸ Weight brush A (model) |
| Tool Details ▸ Edit Glue ▸ Weight brush B | k+ | Modeling ▸ Glue ▸ Weight brush B (model) |
| Tool Details ▸ Edit Glue ▸ Re-merge Glue Vertices Tool | k+ | Modeling ▸ Glue ▸ Re-merge Glue Vertices Tool (model) |
| Tool Details ▸ ArtPath Tools | k+ | similar: tooltip of ArtPath in the toolbar (model) |
| Tool Details ▸ ArtPath Tools ▸ Add control point | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ ArtPath Tools ▸ Delete control point | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ ArtPath Tools ▸ Adjust the opacity set in the control point | k- |  |
| Tool Details ▸ ArtPath Tools ▸ Adjust the thickness set in the control point | k- |  |
| Tool Details ▸ ArtPath Tools ▸ Fold line | k+ | the toolbar ▸ Path (model) |
| Tool Details ▸ ArtPath Tools ▸ Stroke ArtPath | k+ | the toolbar ▸ ArtPath (model) |
| Tool Details ▸ ArtPath Tools ▸ Re-stroke ArtPath | k- |  |
| Tool Details ▸ ArtPath Tools ▸ Join ArtPaths | k- |  |
| Tool Details ▸ ArtPath Tools ▸ Split ArtPath | k+ | similar: tooltip of ArtPath in the toolbar (model) |
| Tool Details ▸ Deform Brush Tool | k- |  |
| Tool Details ▸ Deform Brush Tool ▸ Deform brushes | k+ | Mesh Edit mode ▸ Subtools ▸ Deform brushes (mesh edit) |
| Tool Details ▸ Deform Brush Tool ▸ Expansion Brush | k- |  |
| Tool Details ▸ Deform Brush Tool ▸ Warp Deformer Shaping Brush | k- |  |
| Tool Details ▸ Parameter Controller Tool | k- |  |
| Tool Details ▸ Parameter Controller Tool ▸ Select/Edit Controller | k+ | Deformer ▸ Controllers (animation) |
| Tool Details ▸ Parameter Controller Tool ▸ Add Single Point Controller | k+ | Deformer ▸ Controllers (animation) |
| Tool Details ▸ Parameter Controller Tool ▸ Add Direction Controller | k+ | Deformer ▸ Controllers (animation) |
| Tool Details ▸ Parameter Controller Tool ▸ Delete Controller | k+ | label exists in the app (place not seen by the audit) |
| Tool Details ▸ Parameter Controller Tool ▸ Edit Target | k- |  |
| Add Guide | k+ | similar: Show ▸ Guide ▸ Add Guide to Center (no document) |
| Add Guide ▸ Add a Horizontal Line to Mouse Position | k- |  |
| Add Guide ▸ Add a Vertical Line to Mouse Position | k- |  |
| Add Guide ▸ Add Horizontal/Vertical Lines to Mouse Position | k- |  |
| Palette Common Shortcuts | k- |  |
| Palette Common Shortcuts ▸ Select for Movement | k- |  |
| Palette Common Shortcuts ▸ Move Here | k+ | label exists in the app (place not seen by the audit) |
| Part Shortcuts | k- |  |
| Part Shortcuts ▸ New Part | k+ | Parts ▸ ☰ (mesh edit) |
| Part Shortcuts ▸ Delete selected Parts | k- |  |
| Part Shortcuts ▸ Delete selected Parts | k- |  |
| Part Shortcuts ▸ Disable drag & drop | k+ | Parts ▸ ☰ (mesh edit) |
| Part Shortcuts ▸ Part Settings | k+ | Parts ▸ ☰ (mesh edit) |
| Part Shortcuts ▸ Delete empty parts | k+ | Parts ▸ ☰ (mesh edit) |
| Part Shortcuts ▸ Delete empty parts below child | k- |  |
| Part Shortcuts ▸ Delete empty deformers | k- |  |
| Deformer Shortcuts | k- |  |
| Deformer Shortcuts ▸ Disable drag & drop | k+ | Parts ▸ ☰ (mesh edit) |
| Deformer Shortcuts ▸ Delete empty deformers below child | k- |  |
| Parameter Shortcuts | k- |  |
| Parameter Shortcuts ▸ Cancel Selection | k- |  |
| Parameter Shortcuts ▸ Combine/uncombine | k- |  |
| Parameter Shortcuts ▸ Select Previous | k- |  |
| Parameter Shortcuts ▸ Select Next | k- |  |
| Parameter Shortcuts ▸ Select First Key | k- |  |
| Parameter Shortcuts ▸ Select Next Key | k- |  |
| Parameter Shortcuts ▸ Select Previous Key | k- |  |
| Parameter Shortcuts ▸ Select Last Key | k- |  |
| Parameter Shortcuts ▸ Select Default Key | k- |  |
| Parameter Shortcuts ▸ Select Fist Vertical Key(combined) | k- |  |
| Parameter Shortcuts ▸ Select Next Vertical Key(combined) | k- |  |
| Parameter Shortcuts ▸ Select Previous Vertical Key(combined) | k- |  |
| Parameter Shortcuts ▸ Select Last Vertical Key(combined) | k- |  |
| Parameter Shortcuts ▸ Add Single Keyform | k- |  |
| Parameter Shortcuts ▸ Delete Keyform | k- |  |
| Parameter Shortcuts ▸ Select | k+ | the toolbar ▸ Path (model) |
| Parameter Shortcuts ▸ Flip | k+ | dialog via Modeling ▸ Edit Form ▸ Flip… (model) |
| Parameter Shortcuts ▸ Adjust | k- |  |
| Parameter Shortcuts ▸ Change | k- |  |
| Parameter Shortcuts ▸ Add 2 keys | k+ | context menu of a Parameter row ▸ Add 2 keys (model) |
| Parameter Shortcuts ▸ Add 3 keys | k+ | context menu of a Parameter row ▸ Add 3 keys (model) |
| Parameter Shortcuts ▸ Delete all keys | k+ | context menu of a Parameter row ▸ Delete all keys (model) |
| Parameter Shortcuts ▸ Edit keyform | k- |  |
| Parameter Shortcuts ▸ Display with Parameters of Selected Objects Only | k- |  |
| Parameter Shortcuts ▸ Edit Parameter | k+ | context menu of a Parameter row ▸ Edit Parameter (model) |
| Parameter Shortcuts ▸ Create Parameter | k+ | context menu of a Parameter row ▸ Create Parameter (model) |
| Parameter Shortcuts ▸ Replicate parameters | k- |  |
| Parameter Shortcuts ▸ Create folder | k- |  |
| Parameter Shortcuts ▸ Delete | k+ | Edit ▸ Delete (no document) |
| Parameter Shortcuts ▸ Delete empty parameters | k- |  |
| Parameter Shortcuts ▸ Delete empty parameter groups | k- |  |
| Parameter Shortcuts ▸ Disable drag & drop | k+ | Parts ▸ ☰ (mesh edit) |
| Temporary Snapshots | k- |  |
| Temporary Snapshots ▸ Save | k+ | File ▸ Save (no document) |
| Temporary Snapshots ▸ Save ▸ Save [1] | k+ | File ▸ Save (no document) |
| Temporary Snapshots ▸ Save ▸ Save [2] | k+ | File ▸ Save (no document) |
| Temporary Snapshots ▸ Save ▸ Save [3] | k+ | File ▸ Save (no document) |
| Temporary Snapshots ▸ Save ▸ Save [4] | k+ | File ▸ Save (no document) |
| Temporary Snapshots ▸ Show | k+ | main window (no document) |
| Temporary Snapshots ▸ Show ▸ Show [1] | k+ | main window (no document) |
| Temporary Snapshots ▸ Show ▸ Show [2] | k+ | main window (no document) |
| Temporary Snapshots ▸ Show ▸ Show [3] | k+ | main window (no document) |
| Temporary Snapshots ▸ Show ▸ Show [4] | k+ | main window (no document) |
| Column operations for temporary warp deformations | k- |  |
| Column operations for temporary warp deformations ▸ Add columns | k- |  |
| Column operations for temporary warp deformations ▸ Remove columns | k- |  |
| Column operations for temporary warp deformations ▸ Add rows | k- |  |
| Column operations for temporary warp deformations ▸ Remove rows | k- |  |
| Blend Shape | k+ | label exists in the app (place not seen by the audit) |
| Parameter Bookmarks | k- |  |
| Parameter Bookmarks ▸ Display with Object at Cursor Position | k- |  |
| Parameter Bookmarks ▸ Select from Objects at Cursor Position | k- |  |
| Brush | k+ | main window (mesh edit) |
| Brush ▸ Switch to Previous Brush Shape | k- |  |
| Brush ▸ Switch to Next Brush Shape | k- |  |

