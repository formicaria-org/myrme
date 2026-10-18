# Attribution for the handwriting cases

`eval/cases.json` names photographs, and copies their word annotations, from **GNHK: A Dataset for
English Handwriting in the Wild** (Lee, Huang, Chen et al., ICDAR 2021), by GoodNotes,
https://github.com/GoodNotes/GNHK-dataset. **CC BY 4.0**. The cases were fetched through the
Hugging Face mirror `aryaman27/goodnotes-handwritten-dataset` @
`904297760a9fec813fdc1ecb5f8055cb1ca03550`, which was not checked byte for byte against the
original release. The images are not in this repository: `pixi run eval-data -- handwriting`
fetches them and checks each sha256.
