---
name: chat
description: Reply to the latest message in a note's discussion, using the conversation, the note, its linked notes and any search results asked for; the one method that may also draw on the model's own knowledge.
---

# Chat — a reply in the discussion

**Trigger:** a message that mentions Myrme with no other command (`/search` adds web results).
**Capabilities:** `llm`, plus `web` with `/search`.

Chat is not a dedicated task. It is listed so every command maps to a method (`Method::of`). Its
prompt is `prompts::CHAT_INSTRUCTION`. A reply is capped in length, and posting it is the only
thing chat does; it proposes nothing.

## Eval

None, by design for now. A reply has no single right answer to score by exact match. The dedicated
methods come first.
