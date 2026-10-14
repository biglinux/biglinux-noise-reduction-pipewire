# Security policy

## Reporting a vulnerability

Please do not open a public issue for a vulnerability that could expose audio, execute commands unexpectedly, weaken package isolation, or alter files outside the intended user configuration/runtime directories.

Use GitHub's private security reporting for this repository when available. If private reporting is not available, contact the BigLinux maintainers through the private contact channel listed by the BigLinux organization before publishing technical details.

Include:

- affected version/commit;
- distribution and Python/PipeWire/WirePlumber versions;
- reproduction steps;
- expected versus actual behavior;
- whether microphone audio, command execution, permissions or package integrity are involved.

## Security-sensitive areas

Changes deserve extra review when they touch:

- subprocess execution or executable resolution;
- `/usr/bin` launchers and Python import paths;
- temporary audio files;
- PipeWire module arguments;
- systemd unit hardening;
- settings-file ownership/atomic replacement;
- plugin/shared-library discovery.

The production Python launcher runs in isolated mode (`python3 -I`) and loads application code only from the package-owned private directory. Do not weaken this to make local development imports more convenient.
