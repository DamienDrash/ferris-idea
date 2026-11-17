; IDECraft's colour roles for YAML (SPEC-0027 v13). Placed before the grammar's own query: for the
; same node the earlier pattern wins, and the grammar's query names every scalar a string first, so
; keys looked like their values.

(block_mapping_pair
  key: (flow_node
    (plain_scalar
      (string_scalar) @property)))
(block_mapping_pair
  key: (flow_node
    [
      (double_quote_scalar)
      (single_quote_scalar)
    ] @property))
(flow_mapping
  (_
    key: (flow_node
      (plain_scalar
        (string_scalar) @property))))
(flow_mapping
  (_
    key: (flow_node
      [
        (double_quote_scalar)
        (single_quote_scalar)
      ] @property)))
