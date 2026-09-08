# AI-Assisted Development

OpenCode is installed in the student image as an optional development tool. AI
use is not required, and students must follow the course collaboration,
academic-integrity, and disclosure policies.

## Start OpenCode

From a course repository mounted in the container:

```sh
opencode
```

OpenCode stores global configuration under `~/.config/opencode` and provider
authentication under `~/.local/share/opencode`. The supplied Dev Container
configuration keeps both locations in named volumes so login state survives a
container rebuild without becoming part of the image.

## Recommended Low-Cost Options

Provider availability and pricing can change. Confirm current limits before
recommending a service for a course offering.

1. **Local model through Ollama:** no per-request fee and keeps source on the
   student's machine. Run Ollama on the host and configure OpenCode to use the
   host endpoint through `host.docker.internal`. This requires enough host RAM
   for a model with reliable tool use and a useful context window.
2. **GitHub Copilot Free:** OpenCode supports GitHub Copilot device login through
   `/connect`. The free plan has usage limits, but it avoids requiring students
   to manage a separate API key.
3. **A provider free tier:** OpenCode supports many providers, including
   providers that periodically offer free hosted models. These offers are not a
   stable course dependency.
4. **OpenCode Go or another capped paid account:** suitable when a student wants
   hosted inference with a predictable small budget. Set a provider-side spend
   limit whenever one is available.

The course should recommend local Ollama as the reliably free path and Copilot
Free as the easiest hosted path, while allowing any provider compatible with
the course policy.

Official setup references:

- [OpenCode providers](https://opencode.ai/docs/providers/)
- [OpenCode configuration](https://opencode.ai/docs/config/)
- [OpenCode permissions](https://opencode.ai/docs/permissions/)
- [Ollama integration](https://docs.ollama.com/integrations/opencode)
- [GitHub Copilot plans](https://docs.github.com/en/copilot/get-started/plans-for-github-copilot)

## Authentication Safety

Use OpenCode's `/connect` flow or inject provider credentials at container run
time. Never commit credentials or place them in:

- `container/Dockerfile`
- `container/devcontainer.json`
- A project configuration file
- An image build argument or environment layer

Do not mount an entire home directory, SSH directory, cloud-credential
directory, or Docker socket merely to make AI tooling work. Mount only the
specific configuration or secret needed by the selected provider.

For analysis-only work, prefer read-only workspace access. For implementation
work, review proposed changes, run the project tests, and inspect `git diff`
before submission. Generated code remains the student's responsibility.

## Suggested Student Workflow

1. Read the assignment and identify the smallest current task.
2. Ask the AI tool to explain relevant platform interfaces before requesting
   code.
3. Make one focused change at a time.
4. Run `make`, `make image`, and the released assignment tests.
5. Inspect the diff and remove changes that are unrelated or not understood.
6. Record the interaction and disclose AI use as required by course policy.

AI output should not be treated as an authoritative platform specification. The
released assignment, headers, and versioned `minemu` ABI remain authoritative.
