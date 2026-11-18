# Face motion predictor (Keyform design)

"Auto Generation of Face Motion" (generators.md §3.2) needs, for every
facial feature deformer and every key direction of Angle X / Angle Y, a
displacement of the feature's warp grid. The reference uses trained
machine-learning models for this; Keyform does not copy them. Keyform's
predictor is analytic (accepted in Q-0044):

1. **Face frame.** The Face warp's canvas grid gives the face centre C
   and the face width W.
2. **Feature depth.** Each feature is a plane in front of (or behind) the
   face plane at depth `d = k·W`: eyes and eyeballs 0.15, eyebrows 0.2,
   mouth 0.25, nose 0.45, ears −0.6, the face itself 0.
3. **Rotation.** Every grid point is un-projected at its depth, rotated
   about the point (C, −0.5·W) — half a face width behind the face plane,
   roughly the head's centre — by the key's angle (key / max × 30°), and
   projected back with the perspective model of the 3D Rotation
   Expression (`rotate3d.rs`; camera at C). Corners combine both angles,
   so they need no separate synthesis.
4. **Symmetry.** Left and right features are mirror images in the face
   frame, so the rotation produces mirrored motion without a separate
   rule.
5. **Adjust Face Motion.** For each feature and direction the generated
   displacement field is split into its mean (movement) and the rest
   (deformation); the two sliders scale them (0–200 %, 100 % = as
   generated). Adjustments are always applied to the generated snapshot,
   so they don't compound.

Code: `crates/keyform-edit/src/face.rs`, `rotate3d.rs`.
