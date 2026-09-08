# agentvis

`agentvis` turns local Claude, Codex, and Gemini Tool actions into standalone
repository-evolution artifacts. It consumes the neutral session model from
`agent-session`; repository scoping, Git milestones, layout, and media export
stay in this crate.

The generated visualization is branded **Agent Session Evolution Graph**: files
are stars, repository path areas are stable colors, and Agent actions drive the
timeline.

```bash
cd your-repository
agentvis
```

The default artifact is `output/agent-session-evolution.gif`. Use `-o` to choose
another path or to request additional formats:

```bash
agentvis . --global \
  --compact-rate 30s \
  -o output/agent-session-evolution.html \
  -o output/agent-session-evolution.png \
  -o output/agent-session-evolution.gif \
  -o output/agent-session-evolution.mp4
```

HTML output is a self-contained interactive file. SVG and PNG are still
artifacts; GIF and MP4 replay the same layout frames. AgentSight exposes the
same implementation through `agentsight vis`.

GIF/MP4 default to `--compact-rate 30s`: media frames are selected at uniform
action intervals and encoded at 30 fps. Use `--compact-rate full` to encode
every action frame. HTML always retains every action and ignores media
compaction.

By default, discovery includes every Claude, Codex, and Gemini session whose
cwd, project identity, or Git remote belongs to the worktree. `--global` also
searches sessions rooted elsewhere and retains their absolute-path operations
inside this repository. Each retained Tool action stays on the timeline; an
action with no proven repository file effect produces an unchanged layout
frame instead of disappearing.

To render exactly one session, name its transcript instead of discovering
sessions:

```bash
agentvis . --transcript ~/.claude/projects/<project>/<session>.jsonl --run-id run-123
```

`--transcript` takes one Claude, Codex, Gemini, or Cursor transcript file and
renders that session alone. It conflicts with `--global` (one names a single
file, the other widens discovery), and a path that is missing, unreadable, or
not a recognized transcript is an error rather than an empty graph. `--run-id`
is copied verbatim into the document metadata. The document's `meta` records
`session_scope` (`single_session`, `repository_identity`, or
`global_tool_operations`), `session_id` when the graph holds exactly one
session, and `run_id` when one was given, so a consumer can pair the graph with
that run's terminal recording without parsing the document. `agentsight vis`
accepts the same flags.

## Example

The committed ACTplane example uses the default 30-second action-uniform
compaction: [PNG](https://github.com/eunomia-bpf/agentsight/raw/master/ext/vis/examples/actplane-agent-nebula.png),
[GIF](https://github.com/eunomia-bpf/agentsight/raw/master/ext/vis/examples/actplane-agent-nebula.gif), and
[MP4](https://github.com/eunomia-bpf/agentsight/raw/master/ext/vis/examples/actplane-agent-nebula.mp4).

![Agent Session Evolution Graph visualizing ACTplane](https://github.com/eunomia-bpf/agentsight/raw/master/ext/vis/examples/actplane-agent-nebula.png)
