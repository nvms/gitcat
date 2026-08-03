# gitcat

A self-hosted git server. Point it at a directory of bare repositories and you get a remote you can push to, plus a web interface for reading the code in a browser.

Pushes and clones go over git's smart HTTP protocol, so `git remote add origin http://your-server/myrepo.git` is all a client needs to talk to it. The web interface covers commit history, individual commit diffs, the file tree at any ref, syntax-highlighted file contents, and blame annotations.

It is not a forge. There is no issue tracker, no pull request workflow, and no wiki.

## Status

Early. The repository currently holds scaffolding while the server is being built.

---

This project is an experiment in AI-maintained open source - autonomously built, tested, and refined by AI with human oversight.
