# Motion-sync analyser

Keyform's own audio analyser for the motion-sync preview
(`crates/keyform-anim/src/audio.rs`). The reference editor uses a
third-party engine that Keyform does not have; exported `motionsync3.json`
files are analysed at runtime by the runtime's own engine, so this analyser
only affects Keyform's preview (and, later, baking).

## Input

`Sound` files referenced by `model3.json` motions: RIFF/WAVE with PCM
8/16/24/32-bit integer or 32-bit float samples, any channel count (mixed
to mono). Malformed files are rejected without panicking.

## Per analysis frame

Frames are taken at the setting's `SampleRate` (frames per second). For
each frame time:

1. A 30 ms Hann window centred on the frame time.
2. **Loudness gate**: RMS level in dBFS. Below −45 dB the frame is silent;
   between −45 and −30 dB voicing ramps from 0 to 1. The `Silence` weight is
   1 − voicing.
3. **Formants**: pre-emphasis (0.97), LPC of order 2 + sample rate / 1000
   (Levinson–Durbin on the autocorrelation), and the LPC envelope on a
   10 Hz grid up to 4 kHz. F1 is the strongest envelope peak in
   200–1000 Hz; F2 is the strongest peak at least 300 Hz above F1, up to
   3 kHz.
4. **Vowels**: similarity to five prototypes (F1, F2) — A (750, 1200),
   I (300, 2300), U (350, 1300), E (480, 1900), O (500, 850) — as
   exp(−(ln(F1/F1ᵥ)/0.25)² − (ln(F2/F2ᵥ)/0.20)²), normalised and multiplied
   by voicing.

The output is one weight per element of the default mouth preset:
`Silence`, `A`, `I`, `U`, `E`, `O`.

## From weights to parameters

`Setting::frames` maps the weights through the setting's audio parameters
(scale, clamp, enabled) and mappings to Cubism parameter values, then
applies blend ratio, smoothing and damper. The exact formulas of those
three steps are provisional (question Q-0014). The preview applies the
frames at the sound position as in `70-advanced/viewer.md` §3.8.

## Limitations

- There is no audio output yet: the preview follows the motion's clock as
  if its sound were playing.
- Prototype formants are generic averages; voices far from them (children,
  falsetto) classify less reliably.
