# Native Equipment Slot Placement Audit

The reference item model initializes every item with both hand bits. Legacy `slotType` labels
then mutate that mask: `right-hand` removes the left bit, `left-hand` removes the right bit, and
`two-handed` adds a separate bit. FE correctly retains a bounded unordered set of the parsed
labels, but it deliberately does not retain source order or construct the final TFS slot mask.

The current native transfers also lack a typed receiver/query, slot-compatibility, two-handed
occupancy, cancellation, and generic move-event contract. Enforcing any inferred placement rule
would reject valid legacy content or claim incomplete semantics. No equipment-placement gate is
added by this audit.
