docker: docker runs on ssh Pi4-Server. run all tasks with docker on Pi4-Server via ssh in directory /srv/docker/openadr_lab.

dto: avoid DTO normalization. pass through upstream field names (e.g. OpenADR spec names) across all layers — backend, BFF, UI. one vocabulary everywhere reduces boilerplate and debugging friction.

workflow: 1. always keep a project_journal.md in projects where you write for each large step what you did, why you did it and what issues/key-learnings you had. it shall explain, how the project was implemented. The journal lives at docs/history/project_journal.md.
2. write key learnings into KEY_LEARNINGS.md (at docs/reference/KEY_LEARNINGS.md) and consider them when making decissions.

NEVER stop docker containers that are not involved in this project without asking. They are productive containers. 

When researching about OpenADR reference, only use OpenADR 3 resources. General Questions can be researched from any versions.

Do not add co-authoring footers to commit messages or PR descriptions. they might get rejected.

Only consider upstream PR and commits after the code is tested completely without failure and the commits are ready for the upstream CI acceptance tests.
After creating upstream PR, wait for the CI to actually run and report before drawing any conclusions about main branch being pre-broken. If anything fails, we investigate it properly rather than writing it off.

docs/specs/pdf/: do not read, search, or reference any files under this directory. Use the markdown versions in docs/specs/ instead.