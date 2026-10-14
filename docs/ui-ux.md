# UI/UX principles

The product is primarily a microphone noise-reduction tool for people who may know nothing about audio processing.

## One window

The window follows the BigLinux converters: every setting is a sidebar row that
shows its value and opens a card beside it as the pointer reaches it, or on a
click, tap or Enter for the keyboard. Beside the sidebar, the live panel answers:

1. Is noise reduction working right now?
2. Does my microphone hear me (level meter and spectrum)?
3. How does my voice sound to others (Listen back)?

The sidebar's first group holds the noise reduction (off, a light filter or high
quality), its intensity and the noise that still gets through; the second the
echo cancellation and the microphone in use; the third the voice sound and the
microphone volume. A card that needs room grows for it: Custom shows the
equalizer in the voice sound card, which rises as far as it must to be seen
whole. Measuring the voice and the diagnostics replace the panel until they are
done. A row that cannot apply is grayed and says why in place of its value.

The live panel leads with the state and the microphone and model in use, gives
the rest of its height to the level and spectrum, and ends with Listen back.

Hearing the difference needs no recording: Listen back plays the voice two
seconds later, and turning noise reduction off and on again compares the two.

## Model terminology

The noise reduction row offers Off, Light filter and High quality: names for what they do, with the model behind each (DFN3, DPDFNet) and how much processing it costs in its description; picking one turns the reduction on. DFN3-LL stays available from the command line and shows by its name when chosen there. "Noise that still gets through" (the Silero voice gate) applies to every model, and each keeps its own choice: on by default for the two DFN3 models, off for DPDFNet, which keeps the background quiet itself. "Voice detection" sets how readily that gate hears a voice, from Very sensitive to Very strict (Normal by default); it is disabled while the gate is off, and a change reloads the microphone chain. While the gate runs, the DFN3 silence expander stays off: the gate mutes the pauses itself and keeps word onsets, which the expander took 40-50 ms to reopen for. Command-line DFN3 tunings that differ from the recommended values show a "Manual adjustments active" row with Restore.

Controls that only affect one model are grayed, with the reason, while another model is active.

## Visualizations

The live panel's activity meter is feedback, not an engineering analyzer. Do not expose LUFS, raw FFT scales or editable frequency thresholds on the first screen.

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

## Editors

The EQ and threshold editors are built when their dialog opens and dropped when
it closes. Put Restore after the controls, not before the work it resets. Slider
explanations sit above the slider where a trailing control would squeeze
translated text.

A loaded configuration is not a healthy-audio diagnosis. Explain what is known,
leave latency unknown until a current finite report arrives, and suggest
listening back. Late source-selection results must not navigate away from a
page the user subsequently chose.
