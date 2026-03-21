; Type definitions

(struct_item
  name: (type_identifier) @name) @definition.class

(enum_item
  name: (type_identifier) @name) @definition.class

(value_enum_item
  name: (type_identifier) @name) @definition.class

(type_item
  name: (type_identifier) @name) @definition.class

; Interface definitions

(interface_item
  name: (type_identifier) @name) @definition.interface

; Method definitions (functions inside impl blocks)

(declaration_list
  (function_item
    name: (identifier) @name) @definition.method)

; Function definitions

(function_item
  name: (identifier) @name) @definition.function

; Function calls

(call_expression
  function: (identifier) @name) @reference.call

(call_expression
  function: (field_expression
    field: (field_identifier) @name)) @reference.call

(generic_call_expression
  function: (identifier) @name) @reference.call

(generic_call_expression
  function: (field_expression
    field: (field_identifier) @name)) @reference.call

; Implementations

(impl_item
  type: (type_identifier) @name) @reference.implementation
