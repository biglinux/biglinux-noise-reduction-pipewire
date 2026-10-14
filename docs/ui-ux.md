# UI/UX principles

The product is primarily a microphone noise-reduction tool for people who may know nothing about audio processing.

## Progressive disclosure

The home page should answer:

1. Is noise reduction on?
2. Which microphone is being used?
3. Is my voice level adjusted?
4. How do I test the result?

Technical controls live under Advanced settings.

## One primary action

Each state should have one visually dominant next action. Do not show multiple blue/suggested buttons competing for attention.

Examples:

- uncalibrated microphone → **Adjust volume**;
- calibrated microphone with comparison capability → **Hear the difference**;
- calibrated microphone without A/B capability → **Test microphone**.

## Model terminology

The person chooses one of three models by name: DFN3 (recommended, the lightest), DFN3-LL and DPDFNet. Each choice says in one sentence what it does and how much processing it costs compared with DFN3. "Silence when nobody speaks" (the Silero voice gate) applies to every model, and each keeps its own choice: on by default for the two DFN3 models, off for DPDFNet, which keeps the background quiet itself. "Voice detection" sets how readily that gate hears a voice, from Very sensitive to Very strict (Normal by default); it is disabled while the gate is off, and a change reloads the microphone chain. Command-line DFN3 tunings that differ from the recommended values show a "Manual adjustments active" row with Restore.

Controls that only affect one model must be hidden/clearly disabled when another model is active.

## Visualizations

The home activity meter is feedback, not an engineering analyzer. Do not expose LUFS, raw FFT scales or editable frequency thresholds on the first screen.

Advanced frequency controls must communicate their consequence (for example, sounds below the threshold are silenced) rather than looking like an unrelated EQ curve.

## Accessibility

- Prefer native GTK/libadwaita controls.
- Icon-only buttons need accessible names.
- All primary actions must work by keyboard/touch, not hover only.
- Respect system theme and high contrast.
- Do not communicate state by color alone.
- Reflow at 1024×600 and narrow widths.
- Large text must not hide the only way to complete/cancel a flow.

## Audio safety

Microphone playback/testing should instruct people to use headphones when feedback is possible. Volume calibration must not imply that a high numeric hardware-gain percentage is a quality score.

## Progressive editor layout

Keep the existing microphone spectrum. Create EQ/threshold editors lazily;
collapsed editors must neither paint, take focus nor set the page minimum width.
Group EQ controls as ten on wide pages or two rows of five on narrow pages. Put
Restore after the controls, not before the work it resets. Slider explanations
sit above the slider where a trailing control would squeeze translated text.

A loaded configuration is not a healthy-audio diagnosis. Explain what is known,
leave latency unknown until a current finite report arrives, and encourage a
microphone test. Late source-selection results must not navigate away from a
page the user subsequently chose.
