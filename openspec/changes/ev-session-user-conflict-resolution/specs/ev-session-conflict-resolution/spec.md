# Spec Delta

## Purpose

Turns the refusal `ev-session-queue-foundation` already returns into a decision the
user can act on in one step: the clashing plans are named back to them, and a single
confirmation displaces exactly those and queues the new session.

Refusing a conflicting submission, and stating a session's own availability window,
both belong to the foundation change - without them a user cannot hold two sessions
at all. What is added here is the offer.

## ADDED Requirements

### Requirement: The refusal names what it conflicts with

A refusal SHALL identify every queued session the submission overlaps, carrying
each one's identity, charging window and target, so that the caller can describe
the clash to the user without inspecting the queue separately.

#### Scenario: One conflicting session is named

- **WHEN** a submission overlaps exactly one queued session
- **THEN** the refusal names that session with its window and target

#### Scenario: Several conflicting sessions are named

- **WHEN** a submission spans and overlaps two queued sessions
- **THEN** the refusal names both

#### Scenario: The refusal is distinguishable from other validation failures

- **WHEN** a submission is refused for conflicting with a queued session
- **THEN** the refusal is reported as a conflict, distinctly from an unknown asset, a missing deadline, or a zero-energy target

### Requirement: An explicit replace instruction displaces the named sessions atomically

Where a submission states which conflicting sessions it intends to displace, the
system SHALL remove exactly those sessions and queue the new one as a single
operation. If the insertion cannot complete, no session SHALL have been removed.

#### Scenario: Confirmed replacement succeeds

- **WHEN** a user resubmits the conflicting session naming the session it displaces
- **THEN** the named session is removed, the new session is queued, and no other session is affected

#### Scenario: Replacing several sessions is all or nothing

- **WHEN** a submission conflicts with two queued sessions and names both
- **THEN** both are removed and the new session is queued

#### Scenario: A partial replace instruction is refused

- **WHEN** a submission conflicts with two queued sessions but names only one
- **THEN** the submission is refused, nothing is removed, and the remaining conflict is reported

#### Scenario: A stale replace instruction removes nothing

- **WHEN** a submission names a session that is no longer queued
- **THEN** the submission is refused and no session is removed

#### Scenario: A replace instruction cannot remove a session it does not conflict with

- **WHEN** a submission names a queued session its own window does not overlap
- **THEN** the submission is refused and that session remains queued

### Requirement: The user is offered the replacement rather than the search

The user interface SHALL present a refused conflicting submission as a decision
naming the clashing plan or plans, and SHALL offer to displace them. It SHALL
resubmit with the replace instruction only after the user confirms, and SHALL
leave the queue untouched if the user declines.

#### Scenario: The prompt names the clashing plan

- **WHEN** a user's EV session submission is refused for conflicting with a queued session
- **THEN** the interface shows which plan it clashes with, including that plan's departure, and offers to replace it

#### Scenario: Declining leaves everything in place

- **WHEN** the user declines the offered replacement
- **THEN** no session is created or removed and the user's draft is still available to amend

#### Scenario: Confirming completes the submission

- **WHEN** the user confirms the offered replacement
- **THEN** the clashing plan is gone, the new session appears in the queue, and the user is not asked to find or delete anything themselves

#### Scenario: Several clashing plans are all listed

- **WHEN** a submission clashes with two queued plans
- **THEN** both are named in the prompt and confirming replaces both
