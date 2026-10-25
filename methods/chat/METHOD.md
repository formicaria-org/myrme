---
name: chat
description: Reply to the latest message in a note's discussion, using the conversation, the note, its linked notes and any search results asked for; the one method that may also draw on the model's own knowledge.
---

# Chat — a reply in the discussion

**Trigger:** a message that mentions Myrme with no other command, or `/research` in a first-class
discussion, which adds web results (2026-10-25). **Capabilities:** `llm`, plus `web` there.
**Wait:** class A, a conversation — first words within 2 s, then at least 8 tok/s (`docs/research/2026-10-25-minimum-speed.md` §1). The reply streams to the app's activity as it is written.

Chat is not a dedicated task. It is listed so every command maps to a method (`Method::of`). Its
prompt is `prompts::CHAT_INSTRUCTION`. A reply is capped in length, and posting it is the only
thing chat does; it proposes nothing.

## Eval

None, by design for now. A reply has no single right answer to score by exact match. The dedicated
methods come first.
