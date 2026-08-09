# The testing standard

**A test is not trusted until it has been shown to fail against deliberately
broken code.**

Every rule below was learned by nearly drawing a wrong conclusion. None of them
is theoretical, and each names the case that taught it, because a rule with its
incident attached survives being tidied away.

## 1. Delete the rule, do not perturb it

A perturbed rule can leave behaviour observably identical, and then a green
suite means nothing.

Big Year's all-day anchor read `midnight_of_instant_in_zone(start, tz)`. The
fixture fed it an instant that was **already** midnight in that zone, so both
branches returned the same value and the mutation was invisible. It looked
proved. Deleting the branch outright is what asked the real question.

## 2. Assert the mutation on disk, with a count

Identifiers here routinely occur two or three times. A mutation that applied to
one occurrence, or to none, has repeatedly looked like a proof.

`grep -F` as its own statement, with a count or a line match, **before** the
suite runs. `.query(&[("sendUpdates", send_updates)])` occurs twice — once in
`patch_event`, once in `insert_event` — and an edit aimed at the first landed
wherever the tool found it.

**And run the whole suite: `cargo test --workspace --no-fail-fast`.** Asserting
the mutation is on disk says it applied; running everything is what says who
catches it. Without the flag the run stops after the first failing binary, so a
mutation recorded as caught by two tests was actually caught by five — the three
in `omacal-core` written for that exact function were never reached. That
misreports in the direction that costs: a net that looks narrower than it is,
which is how you come to add tests you already have.

## 3. On disk is not the same as reachable

**A mutation that reddens nothing is either a survivor or an inert edit, and the
two are indistinguishable until you look at what reads the value.**

`attendees_for_edit` builds a `kept` attendee and then calls
`attendee_json(&kept, &a.response_status)` — the status comes from a *separate
argument*, read off the original. Mutating `kept.response_status` was present,
counted, and completely inert; the suite went green and the tests looked weak.
They were not. Mutating the argument reddened eight.

**Corollary.** Where a rule is enforced by the *absence* of code, there is
nothing to delete — the only honest probe is to **add** the thing it forbids,
and to label it an addition rather than dressing it as a deletion. "The
truncation carries `recurrence` alone" is such a rule.

## 4. Restore from a copy, never by an inverse edit

**A revert that is itself a string operation can fail in ways the mutation never
could.**

A mutation whose replacement was the empty string was reverted with
`s.replace("", original, 1)`. In Python that matches at index 0 and *prepended*
the markup to the top of the file. `npm run check` passed — a stray element
before a comment is legal Svelte — and it surfaced later as three
unrelated-looking spec failures.

Copy the file aside first. Move it back. Then `git diff --quiet`.

**Restore, then `touch`, then re-run.** The copy fixes the content and
introduces a second problem: moving a backup back **preserves the backup's
mtime**, which is older than the artifact cargo just compiled from the mutated
source. Cargo then considers its build current and keeps running **the mutant**.
This surfaced as one Rust test failing under `cargo test --workspace` while
passing under `cargo test -p omacal-store` and passing alone — which looks
exactly like test pollution and is not. The two halves are one rule: the copy
restores the bytes, the `touch` restores the build.

## 5. A fixture built from a stated hazard proves the statement, not the code

Twice in one week a mutation caught a **fixture** rather than an implementation.

`CalendarList`'s keyed `{#each}` carried a comment explaining it guarded against
*two accounts subscribed to the same public calendar reporting identical
summaries*. The key sits on the **inner** loop, which iterates within one
account group, so that case is two separate `{#each}` instances and never
collides. What actually throws is **one account holding two calendars with the
same name**. The comment had been wrong since it was written and was quoted
forward, unchallenged, into later briefs — because nothing tested it, only
restated it.

The other: a guest-list test helper built its list from addresses alone and so
invented `optional: false`, silently demoting an attendee who was stored
optional. Every test claiming "the user touched nothing" was asserting something
else.

## 6. A fixture must be able to witness what it claims

Distinct from §5: not a wrong premise, but one that cannot separate.

- *Absolute derivation* needs a zone whose calendar midnight falls on an earlier
  UTC date. New York cannot see it; Auckland can.
- *A skipped or repeated civil hour* is a property of the **browser's** zone,
  not the calendar's.
- *A moved date* needs a drag that crosses midnight — within one day, sending
  the old date is indistinguishable from sending the new one.
- *A duration that changes* needs one that is **not** a multiple of the snap
  interval, or the correct and the naive implementations agree.

And a fixture must reach the code through **the call shape the app actually
makes**. Two fixtures once had the zone right and entered through a signature
nothing called.

## 7. Prove an absence by an absence

"Cancel writes nothing" is witnessed by **no request having been made**, never
by a dialog disappearing or a success value coming back. The same for "a drop
where it started issues no request".

These are the assertions most likely to be quietly weakened later, so they
should be hard to weaken.

## 8. A rule can be genuinely tested and still be false where the test does not reach

A whole-feature rule has as many places to be false as there are code paths
behind it.

"Never mail without asking" was proved on the patch path while `split_series`
still hardcoded `sendUpdates=all` on **both** of its writes — so *Save without
notifying* on a following-scope save would have mailed the guest list twice. The
rule was tested. It was also false, twice, somewhere else.

## 9. Verify gates by exit code

Not by reading output. `grep` over a log has reported "clean" over a real
failure, `${PIPESTATUS[0]}` is empty under zsh, and a stale output file has
described a run that never happened.
