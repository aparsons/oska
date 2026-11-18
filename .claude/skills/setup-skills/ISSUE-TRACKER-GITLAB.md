# Issue tracker: GitLab

This repo tracks issues in GitLab. Use the `glab` CLI for each operation. Inside a clone, `glab` finds the project from the git remote. GitLab calls a comment a "note".

## Check the target project first

Before the first create in a session, run `glab repo view`. Compare the project path with the one that you expect. In a fork, add `--repo <group>/<project>` to each command if needed.

## Basic operations

| Operation | Command |
| --- | --- |
| Create | `glab issue create --title "<title>" --description "$(cat <file>)"` |
| View | `glab issue view <n> --comments` |
| View as JSON | `glab issue view <n> --output json` |
| List | `glab issue list --label "<label>"` (add `--closed` or `--all` for other states) |
| Note | `glab issue note <n> --message "$(cat <file>)"` |
| Add a label | `glab issue update <n> --label "<label>"` |
| Remove a label | `glab issue update <n> --unlabel "<label>"` |
| Create a label | `glab label create --name "<label>" --description "<meaning>"` |

**Close.** `glab issue close` takes no comment. Post the reason as a note first, then close:

```bash
glab issue note <n> --message "$(cat <file>)"
glab issue close <n>
```

## Child issues

Link a child to a parent with an issue link. Else put `Part of #<parent>` on the first line of the child body.

## Blocking

- On paid tiers, put the quick action `/blocked_by #<blocker>` in a note on the blocked issue.
- On other tiers, put `Blocked by: #<a>, #<b>` on the first line of the body.

An issue is unblocked when each blocker is closed.

## Merge requests as a triage surface

**Merge requests are a triage surface: no.**

Change this line to `yes` if the repo treats external merge requests as requests for triage. The `triage` skill reads it. Issues and merge requests have separate numbers: `#<n>` is an issue and `!<n>` is a merge request.

| Operation | Command |
| --- | --- |
| View | `glab mr view <n> --comments` |
| Diff | `glab mr diff <n>` |
| Note | `glab mr note <n> --message "$(cat <file>)"` |
| Labels | `glab mr update <n> --label "<label>"` / `--unlabel "<label>"` |
| Close | Post a note, then `glab mr close <n>` |

To find external authors, compare each author with the project members: `glab api projects/:id/members/all`.

## Generic phrases

| A skill says | Do this |
| --- | --- |
| "Publish to the tracker" | Create an issue. |
| "Fetch the ticket" | View the issue with its notes. |
| "Comment on the ticket" | Add a note. |
| "Close the ticket" | Add a note with the reason, then close. |
| "Apply the label for a role" | Look up the label in `docs/agents/triage-labels.md`, then add it. |

## Wayfinding

- **Map.** One issue with the label `wayfinder:map`. Its body holds the notes, the decisions so far, and the open areas.
- **Child.** An issue linked to the map, with one label `wayfinder:<type>`: `research`, `prototype`, `grilling`, or `task`.
- **Claim.** `glab issue update <n> --assignee @me`, as the first write of the session.
- **Frontier.** Read the links of the map with `glab api projects/:id/issues/<map>/links`. Keep the open children with no open blocker and no assignee. The first one in map order is next.
- **Resolve.** Add a note with the answer, close the child, then add one line to the map body: a short summary and a link.
