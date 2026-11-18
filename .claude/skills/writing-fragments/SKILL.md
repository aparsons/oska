---
name: writing-fragments
description: Interview the user about a topic, such as an article for oskalang.dev, and collect raw writing fragments in one Markdown file.
disable-model-invocation: true
argument-hint: "[file path] [topic]"
---

# writing-fragments

This skill is the explore phase of writing. You interview the user about a topic. You collect raw fragments in one Markdown file. The file has no structure yet.

Structure comes later, with the `writing-beats` or `writing-shape` skill. Do not make outlines, phases, or sections here.

## What a fragment is

A fragment is any piece of text that can survive into the final article. The author must understand it. A cold reader does not need to understand it.

Kinds of fragments:

- A sharp sentence.
- A claim with a short reason.
- A story, an example, an analogy, or a code snippet.
- A half-formed thought.
- A quote, or a line of dialogue.
- A list of related observations.
- A complaint or a joke.
- A key coined word or metaphor that names the central idea. This kind has the most value.

When an idea comes back again and again, push to find a coined word for it. Ask the user: "What do you call this?" Offer two or three names if the user has none.

## Oska articles

Oska articles go to oskalang.dev. `site/build.osk` builds the site from the Markdown under `docs/`, and guides are in `docs/guide/`. The site publishes every page under `docs/`, so a fragments file there gets published.

- Keep the fragments file outside `docs/`. The default is `~/writing/<slug>-fragments.md`. Write down the assumption.
- Capture code fragments as real `.osk` code that the user can run with `oskac run`. Mark a fragment that is not tested yet.
- Capture the words that `GLOSSARY.md` defines with the same spelling and meaning. A fragment that uses a word that it marks `_Avoid_` gets a note.

## File format

- The first write adds only an H1 working title, for example `# Why our deploys are slow (working title)`.
- After the title, the file has fragments only.
- A line with `---` separates two fragments.
- No headings in the body. No tags.
- Fragments stay in the order of capture.

Example body:

```markdown
# Slow deploys (working title)

---

Every deploy waits for the slowest test, not the average test.

---

"We added a cache to make it faster, and now we deploy the cache too."

---

Name: the "convoy". The deploy moves at the speed of its slowest truck.
```

## Steps

### 1. Get the file path

1. If the user gave a path, use it.
2. If not, ask for it one time. If the user is not present, use the default from "Oska articles".
3. Remember the path for the whole session. Do not ask again.

Done when: you have the path of the fragments file.

### 2. Start the file

1. If the file does not exist, write only the H1 working title.
2. If the file exists, read it. Keep all its content.
3. Capture fragments from the conversation so far, starting with the first message. Capture from both sides: the user's words and yours.

Done when: the file exists, has an H1 title, and holds the fragments from the conversation so far.

### 3. Interview the user

Interview in the style of the `grilling` skill:

1. Ask one question at a time.
2. Push on vague answers. Ask for an example, a story, or a number.
3. Follow the ideas that have energy for the user.
4. Look for the central idea, and for a coined word that names it.

Stop when the user says they are done. If the user is not present, stop after you capture the material that you have.

Done when: the user says that the interview is done.

### 4. Capture fragments as you go

After each answer that has usable material:

1. Read the file again. The user can edit it at any time.
2. Keep every edit of the user.
3. Append the new fragments at the end, each after a `---` line.
4. Tell the user in one short line what you added, for example "Added 2 fragments: the convoy metaphor and the cache quote."

Do not ask before each append. Only append. Change an existing fragment only when the user asks.

Done when: each usable piece of the last answer is in the file.

### 5. Obey edit commands at once

When the user gives an edit command, do it at once:

- "Cut that": remove the fragment.
- "Sharpen that": rewrite the fragment to be shorter and more exact. Keep its meaning.
- "Merge those": join the named fragments into one fragment at the place of the first one.

Read the file before the edit. Keep all other content.

Done when: the file shows the edit, and all other content is the same.

## Security

- **Local file.** Keep the fragments file local and outside `docs/`. Do not post, upload, or commit it.
- **Third parties.** Do not add personal data about other people unless the user says so.
