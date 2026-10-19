<!-- The assistant chapter of formicaria's user manual. formicaria keeps no copy: its ci/docs.sh
     renders this file, at the Myrme release it pins, as user/assistant.md of its book — so links
     such as ./collaboration.md point at pages of that book. Headings formicaria links to are part
     of the contract (docs/context/contract.md, part 3a). -->
# The study assistant

formicaria can run a small **local AI assistant** that answers questions in a note's discussion,
researches the web, and drafts note edits for you to review. It is **opt-in and off by default** — a
notebook that never turns it on pays nothing for it — and it runs **entirely on your own device**:
your notes never leave your machine, and the model itself runs locally, so there is no account, no
API key, and no cloud.

It is a *helper*, not an oracle. A small on-device model is good at summarizing, tidying notes, and
fetching-and-summarizing simple facts through a search tool; it will not match a large hosted model on
hard reasoning. That trade — small, private, local — is the point.

## Where it runs

Before anything else, the honest matrix.

| | Assistant | Audio → transcript | Reading images |
|---|---|---|---|
| **Linux**, from the download | **yes** — it fetches what it needs on first enable | **yes** | yes |
| **Windows**, from the download | yes\* | yes\* | yes\* |
| **macOS**, from the download | yes\* | no — see below | yes\* |
| **Linux**, from a checkout | yes | yes | yes |
| **Android** | yes — the runtime is in the app, the model is downloaded when you turn it on | yes, in the app | yes, with the recommended model (since v0.6.4) |

\* **Compiled and type-checked, never yet run.** Linux and Android are the two platforms this has
actually been used on. The per-operating-system parts — chiefly the check that reads how much
memory is free, which the assistant refuses to start a model without — were written on a Linux
machine that cannot compile them, and are exercised only by a CI job that builds them. Until
someone runs it on a Mac or a Windows PC, "yes" there is a well-founded expectation rather than an
observation, and this table would rather say so.

**You no longer need the source code.** Turning the assistant on downloads the model and the runtime
it needs, having first told you how large they are and under what licence. The commands in "Turn it
on" below are the checkout route, which still works and is what a developer wants; a downloaded copy
needs none of them.

**Everything here installs itself**, each as its own choice: the assistant, reading images, and —
on Linux and Windows — turning recordings into text.

**Except transcription on macOS**, where the switch says so rather than appearing and failing. The
speech-to-text runtime is a published build from the whisper.cpp project, and there is no macOS one
at the version formicaria pins. That is not something an installation can fix, which is why the app
says the runtime does not exist for this kind of computer rather than that something is missing from
your machine.

> **macOS and Windows are new here.** Before starting a model, formicaria checks it can read how
> much memory the machine has free, and refuses if that reading fails or looks implausible — it will
> not run a model it cannot watch. That check is what made those platforms wait, and it is still
> what protects them.

Everything else in formicaria — notes, search, boards, backup, sharing — works normally without any
of this.

## Turn it on

### In the app — Linux, macOS or Windows

There is nothing to install first, and no terminal involved.

1. **Settings → Assistant → Study assistant → on.**
2. **It asks which model**, and shows what each one costs: the download size, the licence, and
   whether it can read images. The image reader is a further download and is offered as its own
   choice, because it buys exactly one capability.
3. **It downloads**, showing how far along it is. You can keep working. **Stop** halts it, and what
   already arrived is kept — turning it on again continues rather than starting over.

That is the whole of it. Afterwards the assistant starts with formicaria and stops when you close
it: nothing runs in the background, and nothing is downloaded twice. (With reminders on,
formicaria itself may stay running for one that is waiting; the assistant and its model do not.)

When an update of formicaria brings a newer version of the program that runs the model, the next
start fetches it once, before the assistant comes up, with the same progress shown. If that fails,
the assistant starts on the one it had. A newer *model* is only ever offered, never fetched unasked.

### What it costs, and where it goes

| | |
|---|---|
| **Download** | 0.73 GB to 2.5 GB depending on the model, plus 0.84 GB if you want it to read images. The picker shows the total before it starts. |
| **Disk, afterwards** | The same again — the download *is* the model. Allow a little more while it unpacks the runtime (about 30 MB). |
| **Connection** | Needed once, for that download. Everything after it works offline. |
| **Time** | However long that many gigabytes takes on your connection. You can keep working, and stopping is safe. |

The files live **outside the app folder**, in this computer's configuration directory:

| | |
|---|---|
| Linux | `~/.config/formicaria/tools/` |
| macOS | `~/Library/Application Support/formicaria/tools/` |
| Windows | `%APPDATA%\formicaria\tools\` |

Two consequences worth knowing. Updating formicaria does **not** download it again — the new version
finds it where the old one left it. And copying the app folder to a USB stick takes your notes but
**not** the model, which will be downloaded again on the other machine.

### Getting the space back

Turn the assistant off and delete the model in **Settings → Assistant → Study assistant → Remove the model**. It
tells you how much it will free, asks once, and deletes only the downloaded files — never your
notes. Turning the assistant on afterwards simply asks which model you want again.

Deleting the folder above by hand does the same thing.

### From a checkout, for developers

The app prefers a checkout's `agents/` when it is there, so a working copy needs no download. The
assistant is developed in its own repository,
[formicaria-org/myrme](https://github.com/formicaria-org/myrme), whose tools fetch a model by hand.
In a Myrme checkout:

```sh
pixi run fetch-model              # the lightest default from the catalogue
pixi run fetch-model lfm2.5-1.2b  # a specific, better one (recommended for research)
```

Then link or copy its `agents/runtime` and `agents/models` into this checkout's `agents/`, and run
`pixi run catalogue` here.

Then enable it in Settings as above.

### The two extras

**Transcribing recordings.** Turn on *Audio transcription* under the assistant's own switch. It
downloads about 170 MB — a speech-to-text runtime and its weights — and applies the next time the
assistant starts. It hears **any language** and writes it down in that language — an Italian
recipe comes back in Italian. On macOS the switch is replaced by a line saying no such runtime is
published for that platform yet; that is the whisper.cpp project's build list, not something on
your machine.

**Better listening.** Once transcription is on, a second switch below it, *Better listening*, uses
a larger speech model: about 490 MB more to download, slower (roughly a minute for a two-minute
recording on a laptop), and much closer to what was said — worth it for dictated recipes and long
recordings. Each device chooses for itself. Until the new model has downloaded, the one already
there keeps transcribing; the earlier one is then removed to free the space.

From a Myrme checkout, `pixi run fetch-whisper` still stages the same pieces by hand.

**Reading images.** `/transcribe` can read a photo of a page — handwriting, printed
   text, mathematics as LaTeX — but only with a model that can see. That needs a *projector* file
   beside the weights, and Myrme's `fetch-model` collects it automatically for any model that has one:

   ```sh
   pixi run fetch-model qwen3-vl-4b   # weights + projector; the laptop pick for reading images
   ```

   A text-only model is not an error — the assistant says images are unavailable rather than asking
   a blind model to guess at a picture.

**Web search** works in the app with nothing to set up — but it is **three sources, not the open
web**: **Wikipedia, arXiv and GitHub**, over their public APIs, text only. Each is tried
independently, so one being unreachable does not sink the answer. General web search (DuckDuckGo)
is deliberately not included: it needs fragile HTML scraping of a site that blocks scrapers.

For a research notebook the three cover a great deal, and it is worth knowing what they do not:
a news story, a blog post, a vendor's documentation page. From a Myrme checkout you can run the
keyless local proxy instead, which does reach the general web:

   ```sh
   pixi run search-proxy
   ```

### On Android

The assistant runs **on the phone itself** — the model is bundled in the app and downloads its
weights on first enable.

1. **Build and install the app** (from the project directory, with the Android toolchain set up via
   `pixi run android-init`):

   ```sh
   pixi run android-release          # a signed, 16 KB-aligned APK in mobile/
   ```

   It prints the path it wrote. The name carries the version — `formicaria-v0.5.0-android-arm64.apk`
   from a tagged commit, `formicaria-dev-android-arm64.apk` from any other. Install it on your phone
   (`adb install -r mobile/formicaria-dev-android-arm64.apk`, or copy it across and open it). A **notes-only build** that contains none of the assistant is the default when built
   `--no-default-features` — so people who only want the notebook can ship a lighter app.

2. **First launch** downloads the model (~150 MB–700 MB depending on the model) over your connection,
   shown by a persistent "study assistant" notification. It resumes if interrupted, and only happens
   once. After that the assistant is ready and runs offline.

## Use it

The assistant is called **Myrme** — after the *myrmecologist*, who keeps a formicarium — and that is
its name on every device, whatever model it runs. In a discussion — a note's own discussion thread,
or a first-class discussion — type `@` and the picker suggests it (and any collaborators in the
vault). Pick it and ask:

```
@Myrme what is the difference between mRNA and DNA vaccines?
```

It replies in the thread as **Myrme (laptop)** or **Myrme (phone)** — the copy that answered — so
you can always see who said what. You can give each device's Myrme a name of its own in **Settings
→ Study assistant → This device's Myrme** ("office desktop", "tablet"); `@Myrme-office-desktop` then
asks that one, and it signs as *Myrme (office-desktop)*. The name applies when the assistant next
starts.

**Each device answers its own messages.** If your notes reach a computer and a phone, both run a
Myrme, and a message is answered by the copy on the device you wrote it on — never twice. To ask a
particular one, add its device: `@Myrme-laptop` (or `@Myrme·laptop`) — for something only that
device can do, such as reading a photographed page with the computer's larger model. Its replies are
signed **Myrme (laptop)**, **Myrme (phone)** — the device's name — and what an assistant wrote before
it was called Myrme reads as Myrme too.

**You do not have to remember the commands.** The composer shows them as buttons you can tap, each
with a line saying what it does — which is the easier route on a phone.

| Command | What it does | Where |
|---|---|---|
| `/search` | Look the web up first, then answer from what it found. | Anywhere |
| `/research` | The thorough version: search, write with quotes, and check every claim against its source. Drops the ones it cannot support. | **A note's discussion** |
| `/propose` | Draft an **edit to that note** for you to review. | **A note's discussion** |
| `/transcribe` | Turn this note's **recordings and images into text** — see below. | **A note's discussion** |
| `/recipe` (or `/ricetta`) | Make a **recipe** of this note — a recording of you saying it, or its text — proposed as a new recipe note. | **A note's discussion** |

All but `/search` need a note to work on, so in a *first-class* discussion — one that belongs to no
particular note — only `/search` applies; the rest quietly become an ordinary question. A command
can go anywhere in the sentence, beginning or end.

Nothing the assistant writes lands in a note by itself. `/propose`, `/research`, `/transcribe` and `/recipe`
each produce a **proposal**: it appears on the note it belongs to, and in the **Collaboration**
view, where you see exactly what would change and then accept it, edit it first, or turn it down.
See [Collaboration](./collaboration.md).

Example:

```
@Myrme summarize the key idea of Bayesian model selection in 3 bullet points /search
```

### A recipe from a recording

Record yourself saying a recipe — on the phone, into a note — then, in that note's discussion:

```
@Myrme /recipe
```

Myrme listens to the recording (or reads the note's text, if there is no recording), sorts what you
said into **ingredients**, **method**, **times** and **notes**, and proposes a **new recipe note**.
It uses only what you said: anything it wrote that you did not say is left out, and it tells you
how many lines that was. A word misheard stays misheard — correct it before accepting, or turn on
*Better listening* (above), which mishears far less. The note ends with everything that was heard, under *What was said*, so you
can check it.

On a note that **is already a recipe** — one you dictated in Food, or started yourself — it proposes
the sorted recipe *in that note* instead, keeping its recordings and pictures and quoting what it
said before. If audio transcription is off on this device, it says it did not listen to the
recording and works from the note's text.

Accept it in **Collaboration** — or change it first — and it is a recipe like any other. In **Food**,
its ingredients are then offered one by one to match to foods, so it counts.

### Once it has started, it finishes

**There is no way to stop a reply in progress.** Once the assistant has taken your question there
is no cancel: you wait for it to finish, or you quit the app. The **Stop** button you may have seen
belongs to the *download* on first enable, not to a running answer.

This matters most with `/research`, which is several searches and several passes over what it
found, and can take minutes on a small machine. It is a real gap rather than a design choice —
cancelling means threading a signal down into the model call, which has not been built — so when
you are unsure what you want, ask the short question first.

`/transcribe` turns the media in a note into text: a voice memo becomes a transcript, and a photo of
a page, a whiteboard or a printout becomes something you can search and edit.

```
@Myrme /transcribe
```

Typed on its own it does **everything in the note that has not been done yet** — you never have to
name a file or copy an identifier. Ask twice and it skips what it already read.

For writing it aims at a *usable* transcript, not a description: mathematics comes back as LaTeX
(from the vision model itself — good for ordinary notation, and not a specialised formula reader),
code and pseudocode inside a code block with their indentation, and tables as Markdown tables. For a
chart it transcribes the title, the axis labels, the tick values and the legend.

Three things worth knowing:

- **The picture stays.** The text lands *beside* the image, never instead of it, and says which model
  read it. If the reading is wrong you can always look at the original.
- **It will misread things** — a handwritten symbol, a subscript, an O that is really a zero. Read
  the equations before you trust them. Where it genuinely cannot make something out it writes `[?]`
  rather than guessing, which is the mark to look for.
- **Reading images needs a model that can see.** Recordings need the audio runtime; images need a
  vision model with its projector file fetched. If one of those is missing the assistant says so
  instead of inventing an answer, and still does the half it can.

### What you change is remembered

When you edit or turn down a suggestion, formicaria keeps a record of it: what was suggested, what
you made of it, and — if you tell it — why. There is an optional one-line box for that, and a few
one-tap tags; leaving it empty is fine and costs nothing.

The record stays in your vault with your notes and **is not sent anywhere**. In *Settings* each vault
has two separate switches: whether to keep the record at all, and whether it may ever be shared
openly. The second is off unless you turn it on, and it is asked separately for a reason — agreeing
to keep a note of your own corrections is not agreeing to publish them, and sharing cannot be undone.

### The mention picker

Typing `@` lists everyone you can address here: the assistants that are running, plus the vault's
git **collaborators** (anyone who has posted in a discussion). So `@` works whether you are mentioning
the AI or a person you share the vault with.

## Choosing a model

Each device has a sweet spot — small enough to stay responsive and not slow the rest of the system
down, large enough to answer well. Measured on real hardware:

These are the three the picker offers, with the download sizes it will show you:

| Device | Recommended | Download | Why |
|---|---|---|---|
| **Laptop or desktop** | `qwen3-vl-4b` | 2.5 GB (+0.84 GB to read images) | The default. Grounds its answers best of the ones measured, and the only one that can read a photographed page. |
| **A lighter machine, or a slow connection** | `lfm2.5-1.2b` | 0.73 GB | Best instruction-following and tool-use in its size; the phone's pick until v0.6.4. Text only. |
| **If the default disappoints** | `qwen3-4b-2507` | 2.5 GB | The validated fallback, text-only — the laptop pick until 2026-07-24. |

On a phone there are two, both able to read a picture:

| Phone model | In short | Download |
|---|---|---|
| `lfm2.5-vl-450m-q4` (the default) | Faster | 0.42 GB |
| `qwen3.5-0.8b` | More accurate, slower | 0.71 GB |

### When an update brings a newer model

A new version of formicaria can recommend a different model. **It never downloads one by itself.**
The model you already have keeps working, and **Settings → Assistant → Study assistant** shows a line such as
*"This version of formicaria comes with a newer model for the assistant"*, with its name and the
size of the download.

- **Download** fetches it in the background while you keep working. On a phone the assistant
  switches to it as soon as it has arrived. On a computer it takes over the next time you open
  formicaria. The older model's files are removed once the new one has started.
- **Not now** hides the offer. It comes back only if a later version recommends yet another model.
- **Stop**, while it is downloading, stops the download and leaves the assistant as it was.

Occasionally a version stops supporting an old model altogether. Then the line says the model on
your device no longer works with this version, and downloading the new one is the way to get the
assistant back. Your notes are never affected by any of this.

The assistant caps its own thread use so inference never starves the interface — on a phone this is
also, conveniently, the *fastest* setting.

## If something goes wrong

**"Download failed."** The reason is printed beside it. A dropped connection is the common one and
costs nothing: turn the assistant on again and it continues from where it stopped — what already
arrived is kept.

**"There is not enough free space for this download."** Exactly what it says. Free some space, or
choose the smaller model: `lfm2.5-1.2b` is 0.73 GB against the default's 2.5 GB, and works well.

**The assistant is On, but it never answers.** Almost always memory. The app refuses to start a
model it does not have room for — it will not push your machine into swapping — and the refusal
happens after the switch is already on. Close some applications and browser tabs and try again, or
switch to the smaller model. It needs roughly the model's own size, plus 1 GB, free at the moment
it starts.

**"…cannot read this machine's memory."** formicaria will not run a model it cannot watch, so it
stops rather than guessing. This is unusual; it is worth reporting.

**"No model runtime has been published for this kind of computer yet."** The assistant runs on
64-bit Linux, macOS on Apple silicon, Windows on x86-64, and Android. Other combinations have no
build yet.

**"This copy of formicaria did not come with the assistant."** Some builds are made without it. A
release download from the project's own releases page includes it.

**The transcription switch is missing.** On macOS, expected — no speech-to-text runtime is
published for it. Elsewhere the switch should be there and offer the download; if it is not, the
assistant itself has not been turned on yet.

**Starting over.** Turning the assistant off stops a download in progress and keeps what arrived.
To discard it entirely, use **Remove the model** — see "Getting the space back".

## Privacy and control

- **Off by default.** Nothing runs, downloads, or watches until you enable it.
- **Your notes stay local.** The model runs on your device; notes are never uploaded. Only `/search`
  touches the internet — and only the search terms, never your notes.
- **You are always in control.** The assistant can only **reply** in a discussion or **propose** a
  change on a branch you review. It never writes to `main` on its own. Turn it off in Settings (or,
  on the desktop, simply don't run it) and formicaria is byte-for-byte the plain notebook.
