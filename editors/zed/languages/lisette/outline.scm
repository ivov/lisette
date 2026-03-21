; Functions
(function_item
  "fn" @context
  name: (identifier) @name) @item

; Function signatures (in interfaces)
(function_signature_item
  "fn" @context
  name: (identifier) @name) @item

; Structs
(struct_item
  "struct" @context
  name: (type_identifier) @name) @item

; Enums
(enum_item
  "enum" @context
  name: (type_identifier) @name) @item

(value_enum_item
  "enum" @context
  name: (type_identifier) @name) @item

; Interfaces
(interface_item
  "interface" @context
  name: (type_identifier) @name) @item

; Type aliases
(type_item
  "type" @context
  name: (type_identifier) @name) @item

; Impl blocks
(impl_item
  "impl" @context
  type: (type_identifier) @name) @item

; Constants
(const_item
  "const" @context
  name: (identifier) @name) @item
