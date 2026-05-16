### Requirement: EV session deletion completes linked user requests
When an EV session is deleted, the VEN SHALL transition any UserRequest whose `session_id` matches the deleted session and whose `status` is `Active` to `status: Completed`. The session SHALL be cleared from state after all linked requests are updated.

#### Scenario: No linked request — session is cleared cleanly
- **GIVEN** an active EV session exists with no linked UserRequests
- **WHEN** `DELETE /ev-session` is called
- **THEN** the EV session is cleared from state
- **AND** no UserRequests are modified

#### Scenario: One linked Active request — request is completed
- **GIVEN** an active EV session exists
- **AND** a UserRequest exists with `session_id` matching that session and `status: Active`
- **WHEN** `DELETE /ev-session` is called
- **THEN** the EV session is cleared from state
- **AND** the linked UserRequest has `status: Completed`

#### Scenario: Multiple requests — only Active linked ones are completed
- **GIVEN** an active EV session exists with id `S`
- **AND** a UserRequest exists with `session_id: S` and `status: Active`
- **AND** a UserRequest exists with `session_id: S` and `status: Cancelled`
- **AND** a UserRequest exists with `session_id` of a different session and `status: Active`
- **WHEN** `DELETE /ev-session` is called
- **THEN** only the first UserRequest (Active + matching session) has `status: Completed`
- **AND** the Cancelled request and the other-session Active request are unchanged
