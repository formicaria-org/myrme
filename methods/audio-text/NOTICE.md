# Attribution for the speech cases

`eval/cases.json` names clips and reference transcripts from:

- **Multilingual LibriSpeech** (Pratap et al., 2020), Italian test split, via `facebook/multilingual_librispeech`
  @ `2e83e61823b4c47dcbcb1980bb88601274127609`. **CC BY 4.0**. The recordings are LibriVox
  public-domain audiobooks.
- **LibriSpeech** (Panayotov et al., 2015), test-clean, via `openslr/librispeech_asr`
  @ `71cacbfb7e2354c4226d01e70d77d5fca3d04ba1`. **CC BY 4.0**.

The reference transcripts in `cases.json` are copied from these datasets under CC BY 4.0. The audio
is not in this repository: `pixi run eval-data -- speech` fetches it from the pinned files and
checks each clip's sha256.
