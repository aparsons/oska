# Design it twice

The first interface that you think of is rarely the best one. Design several alternatives in parallel, compare them, and then choose.

## Steps

1. **Write the problem frame.** Write it for the user, in glossary terms. Include these parts:
   - the constraints;
   - each dependency and its category from [DEEPENING.md](DEEPENING.md);
   - a rough sketch of the current code.
   Done when: the user can read the frame without the code.

2. **Start 3 or 4 subagents.** Start them together, each with a fresh context and the problem frame. Give each one a different constraint:
   - **Minimal interface**: 1 to 3 entry points.
   - **Maximum flexibility**: callers can change as much as possible.
   - **Easiest common case**: the most frequent call is one short line.
   - **Ports and adapters**: each outside dependency goes behind a port.
   Do not start more than 4. If a subagent fails, report it and compare the rest.
   Done when: each subagent returned a design, or you recorded its failure.

3. **Collect the same parts from each design.** Each subagent returns:
   - the interface;
   - a usage example;
   - what the implementation hides;
   - the dependency strategy;
   - the trade-offs.
   Done when: every design has all five parts.

4. **Compare.** Compare the designs by depth, by locality, and by seam placement. Use a table with one row per design.
   Done when: the table is complete.

5. **Recommend.** Give one firm recommendation. If two designs each win on a different axis, propose a hybrid and say what it takes from each.
   Done when: the user has one recommended design and the reason for it.
