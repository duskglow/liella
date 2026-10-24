# AI use policy

## How this project was made

Liella was written with [Claude Code](https://claude.com/claude-code), using Anthropic's
Claude models (the `Co-Authored-By` trailer on each commit names the model), under the direction of a human maintainer. The maintainer:

* set the goals;
* made the design decisions and the owner rulings recorded in [plan/](plan/);
* decided how the clean-room rules apply, including the remediation described in
  [docs/clean-room.md](docs/clean-room.md);
* reviewed the results.

Commits co-written with AI carry a `Co-Authored-By` trailer that says so. The record of what
the AI sessions did wrong, including the clean-room lapses, is part of the public history.

## AI-assisted contributions are welcome

You may use AI tools to write code, tests or documentation for Liella. We care about the
quality of a contribution, not about which tools produced it.

## Your contribution is your responsibility

* **You are the author.** Whatever you submit, you are accountable for it: its correctness,
  its licensing, and its consequences. "The AI wrote it" is not an explanation for a bug, a
  hallucinated API, or a broken test. Low-quality contributions will be treated as
  low-quality contributions, whatever tool produced them.
* **Test it and review it yourself before you submit it.** Run the tests your change can
  reach (`cargo xtask affected`). Compare behaviour with a black-box probe where that makes
  sense. Read every line. If you cannot explain what a change does and why it is correct, it
  is not ready.
* **Check the facts.** Anything that claims something about glibc behaviour, the ABI, POSIX or
  the kernel must be verified against the spec, a man page, the POSIX text, the GNU C Library
  manual, or a probe of the installed glibc. Do not trust a model's description alone. A
  model's memory of how glibc does something is exactly what the clean-room rules forbid you
  to use (see below).

## Commit messages and pull requests

* **Descriptive and to the point.** Say what changed and why. Skip the boilerplate,
  filler and marketing tone.
* **Free of hallucinations.** Describe only what the change actually does, how you actually
  tested it, and what you actually observed. Do not claim tests pass that you did not run,
  cite issues that do not exist, or describe behaviour you did not verify.
* **Disclose substantial AI help.** Add a trailer such as
  `Co-Authored-By: <model or tool>` or `Assisted-by: <tool>` to the commit message. It is not
  a penalty. It helps reviewers know where to look harder.

## This project's own rules still apply

AI assistance changes nothing in [CONTRIBUTING.md](CONTRIBUTING.md). In particular:

* **the clean-room rules bind AI tools too.** Do not ask a model to recall, reproduce or
  "port" how glibc, musl, bionic, newlib or a BSD libc implements something, and do not
  accept output that does. If a model's output looks derived from libc source, discard it and
  say so in the pull request. Models are trained on that code, so this needs care from you;
* `unsafe` stays within the documented categories;
* do not paste private or third-party code you have no right to share into an AI service
  you do not control.

Maintainers may ask how a change was produced and tested. They may decline contributions
that show signs of unreviewed generation, such as code that does not match its description,
tests that do not test anything, or confidently wrong explanations.
