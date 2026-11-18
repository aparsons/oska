# Issue tracker: GitHub

This repo tracks issues in GitHub Issues. Use the `gh` CLI for each operation. Inside a clone, `gh` finds the repo from the git remote.

## Check the target repo first

Before the first create in a session, run:

```bash
gh repo view --json nameWithOwner --jq .nameWithOwner
```

Compare the result with the repo that you expect. In a fork, `gh` can default to the parent repo. If the result is wrong, add `--repo <owner>/<name>` to each command.

## Basic operations

Write long bodies to a file first, and pass the file with `--body-file`.

| Operation | Command |
| --- | --- |
| Create | `gh issue create --title "<title>" --body-file <file>` |
| View | `gh issue view <n> --json number,title,body,state,labels,author,assignees,createdAt,updatedAt,comments` |
| List | `gh issue list --state open --label "<label>" --limit 100 --json number,title,labels,author,createdAt,updatedAt` |
| Comment | `gh issue comment <n> --body-file <file>` |
| Add a label | `gh issue edit <n> --add-label "<label>"` |
| Remove a label | `gh issue edit <n> --remove-label "<label>"` |
| Create a label | `gh label create "<label>" --description "<meaning>"` |

Use `--state closed` or `--state all` to list other states. Use `--label` more than one time to require all of the labels.

**Close.** Post the reason as a comment first. Then close:

```bash
gh issue comment <n> --body-file <file>
gh issue close <n> --reason "completed"     # or: --reason "not planned"
```

## Sub-issues

Link a child issue to a parent:

- At create time: `gh issue create --parent <parent> --title "<title>" --body-file <file>`.
- Later: `gh issue edit <parent> --add-sub-issue <child>`.
- If your `gh` has no such flag, use the REST API. It needs the child's database id, not its number:

  ```bash
  child_id=$(gh api repos/{owner}/{repo}/issues/<child> --jq .id)
  gh api --method POST repos/{owner}/{repo}/issues/<parent>/sub_issues -F sub_issue_id="$child_id"
  ```

- If sub-issues are not available, put `Part of #<parent>` on the first line of the child body.

## Blocking

Record that one issue blocks another:

- At create time: `gh issue create --blocked-by <blocker> ...`.
- Later: `gh issue edit <n> --add-blocked-by <blocker>`.
- If your `gh` has no such flag, use the REST API with the blocker's database id:

  ```bash
  blocker_id=$(gh api repos/{owner}/{repo}/issues/<blocker> --jq .id)
  gh api --method POST repos/{owner}/{repo}/issues/<n>/dependencies/blocked_by -F issue_id="$blocker_id"
  ```

- If dependencies are not available, put `Blocked by: #<a>, #<b>` on the first line of the body.

**Count open blockers:**

```bash
gh api repos/{owner}/{repo}/issues/<n> --jq .issue_dependencies_summary.blocked_by
```

This count includes only open blockers. With the text fallback, check each issue in the `Blocked by:` line with `gh issue view <m> --json state`. An issue is unblocked when it has no open blocker.

## Pull requests as a triage surface

**Pull requests are a triage surface: no.**

Change this line to `yes` if the repo treats external pull requests as requests for triage. The `triage` skill reads it. When it is `yes`, use these commands:

| Operation | Command |
| --- | --- |
| View | `gh pr view <n> --json number,title,body,labels,author,authorAssociation,comments` |
| Diff | `gh pr diff <n>` |
| Comment | `gh pr comment <n> --body-file <file>` |
| Labels | `gh pr edit <n> --add-label "<label>"` / `--remove-label "<label>"` |
| Close | `gh pr comment <n> --body-file <file>`, then `gh pr close <n>` |

List open pull requests from external authors:

```bash
gh api --paginate 'repos/{owner}/{repo}/pulls?state=open' \
  --jq '.[] | select(.author_association as $a | ["OWNER","MEMBER","COLLABORATOR"] | index($a) | not)
        | {number, title, author: .user.login, author_association}'
```

Issues and pull requests share one number space on GitHub. To resolve `#<n>`, try `gh pr view <n>` first, then `gh issue view <n>`.

## Generic phrases

| A skill says | Do this |
| --- | --- |
| "Publish to the tracker" | Create an issue. |
| "Fetch the ticket" | View the issue. |
| "Comment on the ticket" | Comment on the issue. |
| "Close the ticket" | Comment with the reason, then close. |
| "Apply the label for a role" | Look up the label in `docs/agents/triage-labels.md`, then add it. |

## Wayfinding

Wayfinding is the work of a large, unclear effort, mapped as one map issue with child issues.

- **Map.** One issue with the label `wayfinder:map`. Its body holds the notes, the decisions so far, and the open areas.
- **Child.** A sub-issue of the map. It has one label `wayfinder:<type>`, where the type is `research`, `prototype`, `grilling`, or `task`.
- **Claim.** Assign the child to yourself: `gh issue edit <n> --add-assignee @me`. Make this the first write of the session.
- **Frontier.** The open children of the map with no open blocker and no assignee. The first one in map order is next. List the children with `gh api repos/{owner}/{repo}/issues/<map>/sub_issues`.
- **Resolve.** Comment with the answer, close the child, then add one line to the map body: a short summary and a link to the child.
