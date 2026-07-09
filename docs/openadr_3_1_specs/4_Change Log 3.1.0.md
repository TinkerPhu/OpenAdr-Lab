# changeLog 

openADR v 3.1.0 

The most significant changes between versions 3.0.1 and 3.1.0 are: 

- message queue notifications (issue 114) 

- object privacy (issues 272, 287, 321, 328) 

- compact representation of serial data like PRICE (issue 238) 

- additional reporting features (issue 271) 

- simpler targeting (316, 331) 

- security hardening (issues 128, 226) 

3.1.0 is NOT fully backwards compatible with 3.0.1 as some changes to API endpoints (e.g. /resources), request bodies (e.g. VENs and resources), and query params (e.g. targets) have been introduced to make the interface more secure, compact, and easier to use. 

The complete list of closed issues is: 

| **GitHub issue** | **Description** |
|---|---|
| 339 | 0000-00-00 not legal FRC3339, change to 0000-01-01 |
| 331 | remove target enumerations from Defintions |
| 328 | generalize description of use of clientID for object privacy |
| 321 | extend target access control to resources |
| 320 | add attributes property to program |
| 316 | refactor targets from valueMap to strings |
| 315 | Recommending mDNS support for local VENs and VTNs |
| 314 | Support sending Price of natural gas in addition to electricity |
| 313 | Add notifier/topics endpoints to support new object-privacy feature |
| 312 | remove targets from /subscriptions queries |
| 311 | Reference enumeration tables (OpenAPI) |
| 310 | Improve description for localPrice field |
| 309 | Contact information (OpenAPI) |
| 306 | make resource a first level object |
| 287 | Allow for Events that are specific and private to a Ven |
| 285 | update OpenADR 3 references in the 3.1.0 documents |
| 272 | limit read scopes for VENs |
| 271 | augment event and report timing features |
| 270 | Correct interpretation for CONTROL_SETPOINT payload type |
| 267 | Event interval payload type to facilitate communicating special dispatch instructions |
| 263 | Add new event interval payloads to maintain parity with Oadr 2.0b |
| 253 | remove programId from report |
| 238 | compact representation of serial event data like PRICES etc |
| 234 | Add query params to filter for active events |
| 231 | make subscription.programID optional: A VEN subscribing to its own ven object |
| 226 | add `/auth/auth_service` endpoint |
| 217 | Change `snake_case` schemas to `camelCase` |
| 209 | change notification operations from http verbs to CRUD |
| 208 | update to version 3.1.0 |
| 207 | replace Word format documentation with markdown |
| 204 | add enumerations_schema folder to 3.1.0 folder |
| 203 | User Guide figure 47 typo |
| 202 | units and typos in User Guide |
| 187 | Restructure OpenAPI spec/schemas to properly define and distinguish between requests and responses |
| 160 | Incorrect definition of the CURVE payload in Definitions |
| 159 | Create schema files describing enumerations |
| 128 | TLS hardening |
| 114 | Pub/Sub: Changes to OpenAPI YAML |

