---
name: wait-what
description: Re-explain the last message in plain, simple English.
disable-model-invocation: true
---

# wait-what

The user did not understand your last message. Explain it again, in a simpler and shorter form.

## Steps

1. **Find the missing context.** Read your previous message. List the terms, the facts, and the steps that the user needs to understand it.
   Done when: you can name what the user did not have.

2. **Find the terms.** Read `GLOSSARY.md`. If `GLOSSARY-MAP.md` exists at the repo root, use it to find the glossary for the topic. Use the glossary terms in your answer. If there is no glossary, use the plainest common word for each concept.
   In the Oska repo, also explain each reference that the previous message used without a gloss. Give a few plain words for each one:
   - an ADR number, such as "ADR 0002 (capabilities are the effect system)";
   - an issue or PR number, with its title;
   - a crate name, such as "`oska-check` (the type checker)";
   - a script or a branch, such as "`scripts/check.sh` (the whole CI test job)".
   Done when: each domain word that you will use matches the glossary, and each reference has a gloss.

3. **Write the explanation.** Put the missing context first. Then state the main point of the previous message again. Write in ASD-STE100 Simplified Technical English:
   - one instruction or one idea per sentence;
   - a maximum of 20 words in an instruction and 25 words in a description;
   - the active voice and simple verb tenses;
   - simple, common words.
   Make it shorter than the original message.
   Done when: the explanation has fewer words than the original.

4. **Say what you need.** End with one line that says what you need from the user. If you need nothing, leave the line out.
   Done when: the user knows the next action, or no action is necessary.

## Security

- **S2 Secrets.** If the previous message had a secret, write `<REDACTED>` in its place.
