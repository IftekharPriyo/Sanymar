# Bundled Kokoro voice notices

Sanymar redistributes the `kokoro-en-v0_19` package obtained from the
Sherpa-ONNX TTS model release. The package README points to the upstream
Kokoro project at <https://huggingface.co/hexgrad/Kokoro-82M>. The package's
Apache License 2.0 text is retained as `LICENSE`.

The `espeak-ng-data` directory originates from eSpeak NG, which identifies
the project as GPL version 3 or later. Its license text is retained as
`LICENSE-espeak-ng-GPL-3.0-or-later.txt`; upstream source and notices are at
<https://github.com/espeak-ng/espeak-ng>.

These files are installed as unmodified runtime assets. Sanymar does not use
them for voice cloning and does not download or replace them at runtime.
