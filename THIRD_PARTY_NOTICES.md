# Third-party notices

## assets/melfilters.bytes, assets/melfilters128.bytes

Mel filter banks (80 and 128 bins) copied unmodified from
https://github.com/huggingface/candle/tree/main/candle-examples/examples/whisper
(commit 12bb5d1f6350f4ea9827d9086f213feec1f0af6a). candle is dual-licensed under
MIT or Apache-2.0; used here under the MIT license:

```
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## assets/icons/subtitles-fill.svg and assets/icons/*.rgba

The tray icon is the Phosphor Icons "subtitles" glyph (fill weight) from
https://github.com/phosphor-icons/core (assets/fill/subtitles-fill.svg),
rasterized to the bundled .rgba files. Phosphor Icons is MIT licensed:

```
MIT License

Copyright (c) 2023 Phosphor Icons

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## src/tts/supertonic.rs

Text preprocessing, chunking and the inference loop are ported (through the Babil extension's
src/tts/supertonic.js) from the sample code of https://github.com/supertone-inc/supertonic,
MIT licensed:

```
MIT License

Copyright (c) 2025 Supertone Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Supertonic 3 voices (downloaded, not bundled)

The Supertonic 3 model and voice styles by Supertone are downloaded on request from
https://huggingface.co/supertone-oss-archive/supertonic-3 and are licensed under the OpenRAIL-M
license (https://huggingface.co/supertone-oss-archive/supertonic-3/blob/main/LICENSE), which
includes use restrictions. The vector estimator comes from
https://huggingface.co/payam1394/traxlate-supertonic-3-q8ve, the same model with that one file
quantized to 8 bits, or from the original repository if that copy is gone.

## ONNX Runtime

Linked into the binaries through the `ort` crate (MIT/Apache-2.0, https://github.com/pykeio/ort).
ONNX Runtime is MIT licensed:

```
MIT License

Copyright (c) Microsoft Corporation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## omnivoice.cpp and ggml (Speaker's voice, `omnivoice` builds only)

Windows builds with the `omnivoice` feature statically link omnivoice.cpp
(https://github.com/ServeurpersoCom/omnivoice.cpp, commit 1a468d8b9d65b3544ca1004d2d7c05dfc65d1f3e,
vendored as the third_party/omnivoice.cpp submodule) and its ggml fork
(https://github.com/ServeurpersoCom/ggml, commit cc5df49a6b878db2b68b9a7b45976e279594627d), both MIT
licensed. omnivoice.cpp ports OmniVoice by k2-fsa (code Apache-2.0) and the Higgs Audio v2 audio
codec by Boson AI (Apache-2.0).

```
MIT License

Copyright (c) 2023-2026 The omnivoice.cpp authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

```
MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## OmniVoice weights (Speaker's voice, downloaded, not bundled)

The OmniVoice model (k2-fsa, https://huggingface.co/k2-fsa/OmniVoice) and its Higgs Audio v2
tokenizer are downloaded on request, in the GGUF conversion for omnivoice.cpp from
https://huggingface.co/Serveurperso/OmniVoice-GGUF (omnivoice-base-Q8_0.gguf and
omnivoice-tokenizer-Q8_0.gguf, revision 017094167b5c9ed565a5076ac9b3b93c5ecf5c73). The weights are
licensed CC-BY-NC-4.0 (https://huggingface.co/k2-fsa/OmniVoice#license): for personal,
non-commercial use only. Hammurabi asks you to agree to that before it downloads them. The Higgs
Audio v2 tokenizer (https://huggingface.co/bosonai/higgs-audio-v2-tokenizer) is Apache-2.0.
