# Todo(s)

1. Double check all the flows and avatar plug ins for Cursor.
2. UI polish, remove the square blur. And make the logo a complete circle. On single click the complete circle is shown.
3. The double click bug.
4. The Windsurf bug.
5. Free move everywhere just like Messenger.
6. SQLite integration or not? *(Waitlist for next release: Database, more agent support in outer orbit, more avatar skins, export Activity for logs. Should confirm these things show up when updated.)* More than the waitlist, why not take them to GitHub issues?
7. Support for other IDE agents.
8. Support for all major platforms.
9. DevOps.
10. Need to find the error avatar.
11. Double check if append is happening or not.
12. Test across IDEs.
13. Architecture refinement.
14. Batch queue in the server.
15. Concurrency check — **[Most important]** What if multiple agents simultaneously...
16. Opt in/out policy for hooks.
17. Analytics for every major feature.
18. Should mostly be fire and forget for observatory hooks. Should not clog agents.
19. Should we introduce WebSockets from runner to server?
20. Work for all platforms.
21. Avatar lifespan. How to track — based on conversation ID?

## Major Product Decisions

- How many agents to show now? How many per IDE?
- How to follow up? Waitlist? Rely only on updates?
- Pricing tier? Feature requests?
- Hooks opt in. IDE opt in/out. IDE enable/disable. Save the previous hook with a specific name. Show in toast that it is named this — e.g. *"Before-SwarmWatch"*
- Any other alternative names?
- If multiple IDEs spwan the avatar, then how will we handle the naming collision.
- Should it be open sourced?
- Should I show the Download count?
- Remove and add feature for avatars.
- Add Cline support and also cline for project-wise support.

## Later offerings

- Analyse the activity logs with AI.
- Analyse with AI, if the work that was done was actually completed.
- Show how much token was wasted in this call. We already know the model used [Imp]
- Become an analytics layer for your agent.
- Or can become the orchestrator for your agents.
- Can become a persistent layer.