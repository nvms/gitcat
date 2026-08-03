# gitcat

[![ci](https://github.com/nvms/gitcat/actions/workflows/ci.yml/badge.svg)](https://github.com/nvms/gitcat/actions/workflows/ci.yml)

A self-hosted git server. Point it at a directory of bare repositories and you get a remote you can push to, plus a web interface for reading the code in a browser.

Pushes and clones go over git's smart HTTP protocol, so `git remote add origin http://your-server/myrepo.git` is all a client needs to talk to it. The web interface covers commit history, individual commit diffs, the file tree at any ref, syntax-highlighted file contents, and blame annotations.

It is not a forge. There is no issue tracker, no pull request workflow, and no wiki.

## Usage

```
gitcat --repos /srv/git --bind 0.0.0.0:9090
```

There is a `Makefile` for local use: `make server` starts it, `make check` runs the format check, lint, and tests, and `make help` lists the rest.

Two kinds of repository under `--repos` are picked up: a bare `myrepo.git` directory, and an ordinary checkout with a `.git` inside it. Create a bare one with `git init --bare /srv/git/myrepo.git`, or point `--repos` at a directory of existing checkouts to browse them in place. Pushing works against bare repositories, since git refuses a push to a branch that is checked out. Options can also be set through `GITCAT_REPOS`, `GITCAT_BIND`, and `GITCAT_SITE_NAME`; log level comes from `GITCAT_LOG`.

## Status

Early. The repository list is browsable and the server runs. Push and clone over smart HTTP, the log and diff views, syntax highlighting, and blame are still being built.

---

This project is an experiment in AI-maintained open source - autonomously built, tested, and refined by AI with human oversight.
