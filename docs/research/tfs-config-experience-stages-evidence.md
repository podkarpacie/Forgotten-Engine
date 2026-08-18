# TFS `config.lua` Experience Stages Evidence

## Public reference

The public TFS `config.lua.dist` documents `experienceStages` as a Lua table. Each entry requires `minlevel` and `multiplier`; `maxlevel` is optional and represents an unbounded final range when absent. It also states that `rateExp` is not used while stages are enabled.[1]

```lua
experienceStages = {
    { minlevel = 1, maxlevel = 8, multiplier = 7 },
    { minlevel = 9, multiplier = 3 }
}
```

## FE clean-room boundary

FE will not execute `config.lua`. The accepted subset is only a bounded literal table assigned directly to `experienceStages`, containing comma-separated entry tables and the three documented unsigned-integer fields. Duplicate or unknown fields, malformed braces, non-literal expressions, overlapping ranges, zero values, and excess input are rejected. A direct `experienceStages = nil` disables stage selection and preserves flat `rateExp` behavior. An accepted non-empty stage table overrides the flat experience rate, matching the documented TFS precedence.

The existing `data/XML/stages.xml` adapter remains supported for FE’s earlier conversion path only when `config.lua` does not declare `experienceStages`. It keeps its existing FE rate-composition behavior to avoid silently changing established worlds.

## References

[1]: https://github.com/otland/forgottenserver/blob/master/config.lua.dist "The Forgotten Server config.lua.dist"
