# Motion-sync analyser

Keyform's motion-sync preview (`crates/keyform-anim/src/audio.rs`,
`crates/keyform-anim/src/motionsync.rs`) follows the pipeline of
`keyform-spec/70-advanced/motion-sync.md` §3. The steps around the
analysis engine are exact. The engine itself is Keyform's own design: the
reference uses a closed third-party engine that Keyform does not have.
Exported `motionsync3.json` files are analysed at runtime by the runtime's
own engine, so this engine only affects Keyform's preview (and, later,
baking).

## Pipeline (spec §3)

1. **Input**: `Sound` files referenced by `model3.json` motions:
   RIFF/WAVE with PCM 8/16/24/32-bit integer or 32-bit float samples, any
   channel count (mixed to mono). Malformed files are rejected without
   panicking. The sound is converted to **16 kHz mono 16-bit** (samples
   s/32768). Keyform resamples by linear interpolation, after a moving
   average about one output sample wide when the source rate is higher.
2. **Engine** (below): one row per 10 ms (**100 rows/s**), with one value
   per Cubism parameter of the setting, or "no value" (NaN).
3. **Resampling** to the setting's `SampleRate`: step q = 100 / SampleRate,
   ⌊rows / q⌋ output rows; row k blends source rows ⌊k·q⌋ and the next one
   (clamped to the last) by the fraction of k·q.
4. **Post-processing**, per Cubism parameter:
   - row 0 is the `Silence` element's targets (none → not written);
   - the *smoothed* and *output* states start at the Silence targets;
   - "no value" writes nothing and clears both states; the first value
     after that is taken as is;
   - otherwise v = ((100 − Smooth)·x + Smooth·smoothed)/100,
     smoothed = v; if |v − output| < Damper the output is held, else it
     becomes v; the output is written.
5. **Preview**: the rows are applied at the sound position as in
   `70-advanced/viewer.md` §3.8.

## Engine (Keyform's design, spec §4)

### Element weights

Per 10 ms frame:

1. A 30 ms Hann window centred on the frame time.
2. **Loudness gate**: RMS level in dBFS. Below −45 dB the frame is silent;
   between −45 and −30 dB voicing ramps from 0 to 1. The voicing is the
   frame's **level**, and the `Silence` weight is 1 − voicing.
3. **Formants**: pre-emphasis (0.97), LPC of order 2 + sample rate / 1000
   (Levinson–Durbin on the autocorrelation), and the LPC envelope on a
   10 Hz grid up to 4 kHz. F1 is the strongest envelope peak in
   200–1000 Hz; F2 is the strongest peak at least 300 Hz above F1, up to
   3 kHz.
4. **Vowels**: similarity to five prototypes (F1, F2): A (750, 1200),
   I (300, 2300), U (350, 1300), E (480, 1900), O (500, 850). The
   similarity is exp(−(ln(F1/F1ᵥ)/0.25)² − (ln(F2/F2ᵥ)/0.20)²), normalised
   and multiplied by voicing.

This gives one weight per element of the default mouth preset: `Silence`,
`A`, `I`, `U`, `E`, `O`.

### Engine inputs from the setting

- Elements are the enabled audio parameters that have a Shape mapping.
  Each element's weight is multiplied by its `Scale` (negative results
  count as 0). The audio parameters' `Min`/`Max` are not engine inputs and
  are ignored, as in the reference.
- An element without a target for a Cubism parameter doesn't contribute to
  it. A parameter that no element targets gets "no value".

### Per parameter and frame

1. **Mix**: mix = Σ wₑ·targetₑ / Σ wₑ over the contributing elements. If
   Σ wₑ = 0, the result is the Silence target (or "no value" without one).
2. **BlendRatio** r (percent): result = one + (mix − one)·r/100, where *one*
   is the target of the strongest contributing element. At 0 the shape
   snaps to the dominant vowel; at 100 it is the weighted mix.
3. **AudioLevelEffectRatio** a (percent, default 0): with a Silence target
   s₀, result = s₀ + (result − s₀)·(1 − a/100 + a/100·level). Quiet frames
   pull the shape toward Silence in proportion to a.
4. **Smoothing** S (0–100): y = ((100 − S)·result + S·y_prev)/100, the same
   form as the per-parameter Smooth. "No value" resets it.

## Limitations

- There is no audio output yet: the preview follows the motion's clock as
  if its sound were playing.
- Prototype formants are generic averages; voices far from them (children,
  falsetto) classify less reliably.
