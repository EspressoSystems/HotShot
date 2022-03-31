# Best Practices

- No pushing to main at all, only through pull requests
  - Releases are a pull request with just the release version bump, release after this is merged into main: https://github.com/bincode-org/bincode/pull/510
- Only append commits to a pull request, don't rebase/squash/force push anything because this makes reviewing more annoying
  - Force pushing when rebasing onto main is fine, but merge commits are also fine
- Reviewers should only review/approve, let the creator of the PR complete the PR
- Always squash & merge when completing your pull request
  - The full history will be in the pull request
  - The main branch will have a single commit with the summary of the PR with a link if you want more details

