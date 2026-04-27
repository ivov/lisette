/**
 * @file Lisette grammar for tree-sitter
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  closure: -1,
  assign: 0,
  pipeline: 1,
  or: 2,
  and: 3,
  comparative: 4,
  range: 5,
  additive: 6,
  multiplicative: 7,
  cast: 8,
  unary: 9,
  call: 10,
  field: 10,
  try: 10,
};

const ESCAPE_SEQ = choice(
  /[^xu]/,
  /u[0-9a-fA-F]{4}/,
  /u\{[0-9a-fA-F]+\}/,
  /x[0-9a-fA-F]{2}/,
);

/**
 * @param {RuleOrLiteral} sep
 * @param {RuleOrLiteral} rule
 * @returns {SeqRule}
 */
function sepBy1(sep, rule) {
  return seq(rule, repeat(seq(sep, rule)));
}

/**
 * @param {RuleOrLiteral} sep
 * @param {RuleOrLiteral} rule
 * @returns {ChoiceRule}
 */
function sepBy(sep, rule) {
  return optional(sepBy1(sep, rule));
}

/** @param {RuleOrLiteral} rule */
function commaSep(rule) {
  return seq(sepBy(',', rule), optional(','));
}

/** @param {RuleOrLiteral} rule */
function bracedList(rule) {
  return seq('{', commaSep(rule), '}');
}

/** @param {RuleOrLiteral} rule */
function parenList(rule) {
  return seq('(', commaSep(rule), ')');
}

module.exports = grammar({
  name: 'lisette',

  extras: $ => [
    /\s/,
    $.line_comment,
  ],

  externals: $ => [
    $.string_content,
    $.float_literal,
    $._automatic_semicolon,
    $._format_string_content,
    $._error_sentinel,
    $._open_angle,
    $._bang,
  ],

  supertypes: $ => [
    $._expression,
    $._type,
    $._literal,
    $._literal_pattern,
    $._declaration_statement,
    $._pattern,
  ],

  inline: $ => [
    $._field_identifier,
    $._declaration_statement,
    $._expression_ending_with_block,
  ],

  conflicts: $ => [
    [$.unit_type, $.tuple_pattern],
    [$._type, $.scoped_type_identifier],
    [$._expression_except_range, $._type_identifier],
    [$._pattern, $._type_identifier],
  ],

  word: $ => $.identifier,

  rules: {
    source_file: $ => repeat($._statement),

    _statement: $ => choice(
      $.expression_statement,
      $._declaration_statement,
    ),

    empty_statement: _ => ';',

    _semicolon: $ => choice(';', $._automatic_semicolon),

    expression_statement: $ => choice(
      seq($._expression, $._semicolon),
      prec(1, $._expression_ending_with_block),
    ),

    _declaration_statement: $ => choice(
      $.const_item,
      $.empty_statement,
      $.attribute_item,
      $.struct_item,
      $.enum_item,
      $.value_enum_item,
      $.type_item,
      $.function_item,
      $.function_signature_item,
      $.impl_item,
      $.interface_item,
      $.let_declaration,
      $.var_declaration,
      $.import_declaration,
      $.defer_statement,
      $.task_statement,
    ),

    // Attributes

    attribute_item: $ => seq(
      '#',
      '[',
      $.attribute,
      ']',
    ),

    attribute: $ => seq(
      field('name', $.identifier),
      optional(field('arguments', $.attribute_arguments)),
    ),

    attribute_arguments: $ => parenList($._attribute_arg),

    _attribute_arg: $ => choice(
      $.identifier,                           // flag: omitempty, snake_case
      $.string_literal,                       // string: "fieldName"
      $.backtick_literal,                     // raw: `validate:"required"`
      seq('!', $.identifier),                 // negated flag: !omitempty
    ),

    // Imports

    import_declaration: $ => seq(
      'import',
      optional(field('alias', choice(
        $.identifier,
        '_',
      ))),
      field('path', $.string_literal),
      $._semicolon,
    ),

    // Declarations

    struct_item: $ => seq(
      optional($.visibility_modifier),
      'struct',
      field('name', $._type_identifier),
      field('type_parameters', optional($.type_parameters)),
      choice(
        field('body', $.field_declaration_list),
        field('body', $.ordered_field_declaration_list),
      ),
    ),

    enum_item: $ => seq(
      optional($.visibility_modifier),
      'enum',
      field('name', $._type_identifier),
      field('type_parameters', optional($.type_parameters)),
      field('body', $.enum_variant_list),
    ),

    value_enum_item: $ => seq(
      optional($.visibility_modifier),
      'enum',
      field('name', $._type_identifier),
      ':',
      field('underlying_type', $._type),
      field('body', $.value_enum_variant_list),
    ),

    value_enum_variant_list: $ => bracedList(seq(
      optional($.doc_comment),
      $.value_enum_variant,
    )),

    value_enum_variant: $ => seq(
      field('name', $.identifier),
      '=',
      field('value', choice(
        $.integer_literal,
        $.negative_literal,
        $.string_literal,
      )),
    ),

    enum_variant_list: $ => bracedList(seq(
      repeat($.attribute_item), optional($.doc_comment), $.enum_variant,
    )),

    enum_variant: $ => seq(
      field('name', $.identifier),
      field('body', optional(choice(
        $.field_declaration_list,
        $.ordered_field_declaration_list,
      ))),
    ),

    field_declaration_list: $ => bracedList(seq(
      repeat($.attribute_item),
      optional($.doc_comment),
      $.field_declaration,
    )),

    field_declaration: $ => seq(
      optional($.visibility_modifier),
      field('name', $._field_identifier),
      ':',
      field('type', $._type),
    ),

    ordered_field_declaration_list: $ => parenList(seq(
      repeat($.attribute_item),
      optional($.visibility_modifier),
      field('type', $._type),
    )),

    const_item: $ => seq(
      optional($.visibility_modifier),
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      $._semicolon,
    ),

    var_declaration: $ => seq(
      optional($.visibility_modifier),
      'var',
      field('name', $.identifier),
      ':',
      field('type', $._type),
      $._semicolon,
    ),

    type_item: $ => seq(
      optional($.visibility_modifier),
      'type',
      field('name', $._type_identifier),
      field('type_parameters', optional($.type_parameters)),
      optional(seq('=', field('type', $._type))),
      $._semicolon,
    ),

    function_item: $ => seq(
      optional($.visibility_modifier),
      'fn',
      field('name', $.identifier),
      field('type_parameters', optional($.type_parameters)),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block),
    ),

    function_signature_item: $ => seq(
      optional($.visibility_modifier),
      'fn',
      field('name', $.identifier),
      field('type_parameters', optional($.type_parameters)),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
      $._semicolon,
    ),

    impl_item: $ => seq(
      'impl',
      field('type_parameters', optional($.type_parameters)),
      field('type', choice(
        $._type_identifier,
        $.scoped_type_identifier,
        $.generic_type,
      )),
      field('body', $.declaration_list),
    ),

    interface_item: $ => seq(
      optional($.visibility_modifier),
      'interface',
      field('name', $._type_identifier),
      field('type_parameters', optional($.type_parameters)),
      field('body', $.interface_body),
    ),

    interface_body: $ => seq(
      '{',
      repeat(choice(
        $.function_signature_item,
        $.interface_embedding,
      )),
      '}',
    ),

    interface_embedding: $ => seq(
      'impl',
      field('interface', $._type),
      $._semicolon,
    ),

    declaration_list: $ => seq(
      '{',
      repeat($._declaration_statement),
      '}',
    ),

    // Parameters

    parameters: $ => parenList(seq(
      optional($.attribute_item),
      choice(
        $.parameter,
        $.self_parameter,
        '_',
        $._type,
      ),
    )),

    self_parameter: $ => 'self',

    parameter: $ => seq(
      optional($.mutable_specifier),
      field('pattern', choice(
        $._pattern,
        'self',
      )),
      ':',
      field('type', $._type),
    ),

    visibility_modifier: _ => 'pub',

    mutable_specifier: _ => 'mut',

    // Type parameters

    type_parameters: $ => prec(1, seq(
      '<',
      sepBy1(',', seq(
        repeat($.attribute_item),
        $.type_parameter,
      )),
      optional(','),
      '>',
    )),

    type_parameter: $ => prec(1, seq(
      field('name', $._type_identifier),
      optional(field('bounds', $.trait_bounds)),
    )),

    trait_bounds: $ => seq(
      ':',
      sepBy1('+', $._type),
    ),

    // Types

    _type: $ => choice(
      $.generic_type,
      $.scoped_type_identifier,
      $.tuple_type,
      $.unit_type,
      $.function_type,
      $._type_identifier,
      $.never_type,
    ),

    generic_type: $ => prec(1, seq(
      field('type', choice(
        $._type_identifier,
        $.scoped_type_identifier,
      )),
      field('type_arguments', $.type_arguments),
    )),

    scoped_type_identifier: $ => prec.left(seq(
      field('path', choice(
        $._type_identifier,
        $.scoped_type_identifier,
        $.generic_type,
      )),
      '.',
      field('name', $._type_identifier),
    )),

    type_arguments: $ => prec(-1, seq(
      '<',
      sepBy1(',', choice(
        $._type,
        $._literal,
      )),
      optional(','),
      '>',
    )),

    function_type: $ => seq(
      'fn',
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
    ),

    tuple_type: $ => seq(
      '(',
      sepBy1(',', $._type),
      optional(','),
      ')',
    ),

    unit_type: _ => seq('(', ')'),

    never_type: _ => 'Never',

    // Expressions

    _expression_except_range: $ => choice(
      $.unary_expression,
      $.reference_expression,
      $.try_expression,
      $.binary_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.type_cast_expression,
      $.call_expression,
      $.generic_call_expression,
      $.return_expression,
      $._literal,
      prec.left($.identifier),
      $.self,
      $.field_expression,
      $.slice_expression,
      $.tuple_expression,
      $.unit_expression,
      $.break_expression,
      $.continue_expression,
      $.index_expression,
      $.closure_expression,
      $.parenthesized_expression,
      $.struct_expression,
      $.deref_expression,
      $.rawgo_directive,
      $._expression_ending_with_block,
    ),

    _expression: $ => choice(
      $._expression_except_range,
      $.range_expression,
    ),

    _expression_ending_with_block: $ => choice(
      $.block,
      $.if_expression,
      $.if_let_expression,
      $.match_expression,
      $.while_expression,
      $.while_let_expression,
      $.loop_expression,
      $.for_expression,
      $.select_expression,
      $.try_block,
      $.recover_block,
    ),

    // Binary / unary / postfix expressions

    range_expression: $ => prec.left(PREC.range, choice(
      seq($._expression, choice('..', '..='), $._expression),
      seq($._expression, '..'),
      seq('..', $._expression),
      seq('..=', $._expression),
      '..',
    )),

    unary_expression: $ => prec(PREC.unary, seq(
      choice('-', alias($._bang, '!')),
      $._expression,
    )),

    reference_expression: $ => prec(PREC.unary, seq(
      '&',
      field('value', $._expression),
    )),

    try_expression: $ => prec(PREC.try, seq(
      $._expression,
      '?',
    )),

    deref_expression: $ => prec(PREC.field, seq(
      field('value', $._expression),
      '.',
      '*',
    )),

    binary_expression: $ => {
      const table = [
        [PREC.pipeline, '|>'],
        [PREC.and, '&&'],
        [PREC.or, '||'],
        [PREC.comparative, choice('==', '!=', '<=', '>=')],
        [PREC.additive, choice('+', '-')],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];

      return choice(
        // @ts-ignore
        ...table.map(([precedence, operator]) => prec.left(precedence, seq(
          field('left', $._expression),
          // @ts-ignore
          field('operator', operator),
          field('right', $._expression),
        ))),
        prec.left(PREC.comparative, seq(
          field('left', $._expression),
          field('operator', '<'),
          field('right', $._expression),
        )),
        prec.left(PREC.comparative, seq(
          field('left', $._expression),
          field('operator', '>'),
          field('right', $._expression),
        )),
      );
    },

    assignment_expression: $ => prec.left(PREC.assign, seq(
      field('left', $._expression),
      '=',
      field('right', $._expression),
    )),

    compound_assignment_expression: $ => prec.left(PREC.assign, seq(
      field('left', $._expression),
      field('operator', choice('+=', '-=', '*=', '/=', '%=')),
      field('right', $._expression),
    )),

    type_cast_expression: $ => prec.left(PREC.cast, seq(
      field('value', $._expression),
      'as',
      field('type', $._type),
    )),

    return_expression: $ => choice(
      prec.left(seq('return', $._expression)),
      prec(-1, 'return'),
    ),

    call_expression: $ => prec(PREC.call, seq(
      field('function', $._expression_except_range),
      field('arguments', $.arguments),
    )),

    generic_call_expression: $ => prec(PREC.call, seq(
      field('function', choice(
        $.identifier,
        $.field_expression,
      )),
      field('type_arguments', $.call_type_arguments),
      field('arguments', $.arguments),
    )),

    call_type_arguments: $ => seq(
      $._open_angle,
      sepBy1(',', choice($._type, $._literal)),
      optional(','),
      '>',
    ),

    arguments: $ => parenList($._expression),

    slice_expression: $ => seq('[', commaSep($._expression), ']'),

    parenthesized_expression: $ => seq(
      '(',
      $._expression,
      ')',
    ),

    tuple_expression: $ => seq(
      '(',
      seq($._expression, ','),
      repeat(seq($._expression, ',')),
      optional($._expression),
      ')',
    ),

    unit_expression: _ => seq('(', ')'),

    struct_expression: $ => seq(
      field('name', choice(
        $._type_identifier,
        $.scoped_type_identifier,
      )),
      field('body', $.field_initializer_list),
    ),

    field_initializer_list: $ => bracedList(choice(
      $.shorthand_field_initializer,
      $.field_initializer,
      $.base_field_initializer,
      $.zero_fill_initializer,
    )),

    shorthand_field_initializer: $ => $.identifier,

    field_initializer: $ => seq(
      field('field', choice($._field_identifier, $.integer_literal)),
      ':',
      field('value', $._expression),
    ),

    base_field_initializer: $ => seq(
      '..',
      $._expression,
    ),

    zero_fill_initializer: _ => prec(1, '..'),

    // Control flow

    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $._expression),
      field('consequence', $.block),
      optional(field('alternative', $.else_clause)),
    )),

    if_let_expression: $ => prec.right(seq(
      'if',
      'let',
      field('pattern', $._pattern),
      '=',
      field('value', $._expression),
      field('consequence', $.block),
      optional(field('alternative', $.else_clause)),
    )),

    else_clause: $ => seq(
      'else',
      choice(
        $.block,
        $.if_expression,
        $.if_let_expression,
      ),
    ),

    match_expression: $ => seq(
      'match',
      field('value', $._expression),
      field('body', $.match_block),
    ),

    match_block: $ => seq(
      '{',
      optional(seq(
        repeat($.match_arm),
        alias($.last_match_arm, $.match_arm),
      )),
      '}',
    ),

    match_arm: $ => prec.right(seq(
      field('pattern', $.match_pattern),
      '=>',
      choice(
        seq(field('value', $._expression), ','),
        field('value', prec(1, $._expression_ending_with_block)),
      ),
    )),

    last_match_arm: $ => seq(
      field('pattern', $.match_pattern),
      '=>',
      field('value', $._expression),
      optional(','),
    ),

    match_pattern: $ => seq(
      $._pattern,
      optional(seq('if', field('condition', $._expression))),
    ),

    while_expression: $ => seq(
      'while',
      field('condition', $._expression),
      field('body', $.block),
    ),

    while_let_expression: $ => seq(
      'while',
      'let',
      field('pattern', $._pattern),
      '=',
      field('value', $._expression),
      field('body', $.block),
    ),

    loop_expression: $ => seq(
      'loop',
      field('body', $.block),
    ),

    for_expression: $ => seq(
      'for',
      field('pattern', $._pattern),
      'in',
      field('value', $._expression),
      field('body', $.block),
    ),

    break_expression: $ => prec.left(seq('break', optional($._expression))),

    continue_expression: _ => prec.left('continue'),

    // Concurrency

    task_statement: $ => choice(
      prec(1, seq('task', $.block)),
      seq('task', $._expression, $._semicolon),
    ),

    defer_statement: $ => choice(
      prec(1, seq('defer', $.block)),
      seq('defer', $._expression, $._semicolon),
    ),

    select_expression: $ => seq(
      'select',
      bracedList($.select_arm),
    ),

    select_arm: $ => choice(
      $.select_receive_arm,
      $.select_send_arm,
      $.select_match_arm,
      $.select_default_arm,
    ),

    select_receive_arm: $ => seq(
      'let',
      field('pattern', $._pattern),
      '=',
      field('channel', $._expression),
      '=>',
      field('body', $._expression),
    ),

    select_send_arm: $ => seq(
      field('send', $._expression),
      '=>',
      field('body', $._expression),
    ),

    select_match_arm: $ => seq(
      'match',
      field('value', $._expression),
      field('body', $.match_block),
    ),

    select_default_arm: $ => seq(
      '_',
      '=>',
      field('body', $._expression),
    ),

    // Error handling

    try_block: $ => seq(
      'try',
      $.block,
    ),

    recover_block: $ => seq(
      'recover',
      $.block,
    ),

    // Closures

    closure_expression: $ => prec(PREC.closure, seq(
      field('parameters', $.closure_parameters),
      choice(
        seq(
          optional(seq('->', field('return_type', $._type))),
          field('body', $.block),
        ),
        field('body', $._expression),
      ),
    )),

    closure_parameters: $ => seq(
      '|',
      sepBy(',', choice(
        $._pattern,
        $.parameter,
      )),
      '|',
    ),

    // Field / index access

    index_expression: $ => prec(PREC.call, seq(
      $._expression,
      '[',
      $._expression,
      ']',
    )),

    field_expression: $ => prec(PREC.field, seq(
      field('value', $._expression),
      '.',
      field('field', choice(
        $._field_identifier,
        $.integer_literal,
      )),
    )),

    // Blocks

    block: $ => seq(
      '{',
      repeat($._statement),
      optional($._expression),
      '}',
    ),

    // Let / var

    let_declaration: $ => seq(
      'let',
      optional($.mutable_specifier),
      field('pattern', $._pattern),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      optional(seq(
        'else',
        field('alternative', $.block),
      )),
      $._semicolon,
    ),

    // Directives

    rawgo_directive: $ => seq(
      '@rawgo',
      '(',
      $.string_literal,
      ')',
    ),

    // Patterns

    _pattern: $ => choice(
      $._literal_pattern,
      $.identifier,
      $.tuple_pattern,
      $.tuple_struct_pattern,
      $.struct_pattern,
      $.slice_pattern,
      $.or_pattern,
      $.as_binding_pattern,
      $.remaining_field_pattern,
      '_',
    ),

    as_binding_pattern: $ => prec.left(seq(
      field('pattern', $._pattern),
      'as',
      field('name', $.identifier),
    )),

    tuple_pattern: $ => parenList(choice($._pattern, $.closure_expression)),

    slice_pattern: $ => seq('[', commaSep($._pattern), ']'),

    tuple_struct_pattern: $ => seq(
      field('type', choice(
        $.identifier,
        $.scoped_type_identifier,
      )),
      parenList($._pattern),
    ),

    struct_pattern: $ => seq(
      field('type', choice(
        $._type_identifier,
        $.scoped_type_identifier,
      )),
      bracedList(choice($.field_pattern, $.remaining_field_pattern)),
    ),

    field_pattern: $ => choice(
      field('name', alias($.identifier, $.shorthand_field_identifier)),
      seq(
        field('name', $._field_identifier),
        ':',
        field('pattern', $._pattern),
      ),
    ),

    remaining_field_pattern: $ => seq(
      '..',
      optional($.identifier),
    ),

    or_pattern: $ => prec.left(-2, seq(
      $._pattern,
      '|',
      $._pattern,
    )),

    // Literals

    _literal: $ => choice(
      $.string_literal,
      $.raw_string_literal,
      $.char_literal,
      $.boolean_literal,
      $.integer_literal,
      $.float_literal,
      $.imaginary_literal,
      $.format_string,
    ),

    _literal_pattern: $ => choice(
      $.string_literal,
      $.raw_string_literal,
      $.char_literal,
      $.boolean_literal,
      $.integer_literal,
      $.float_literal,
      $.negative_literal,
    ),

    negative_literal: $ => seq('-', choice($.integer_literal, $.float_literal)),

    integer_literal: _ => token(seq(
      choice(
        /[0-9][0-9_]*/,
        /0x[0-9a-fA-F_]+/,
        /0b[01_]+/,
        /0o[0-7_]+/,
      ),
    )),

    imaginary_literal: $ => seq(
      choice($.integer_literal, $.float_literal),
      token.immediate('i'),
    ),

    string_literal: $ => seq(
      '"',
      repeat(choice(
        $.escape_sequence,
        $.string_content,
      )),
      token.immediate('"'),
    ),

    raw_string_literal: _ => token(seq('r"', /[^"\n]*/, '"')),

    format_string: $ => seq(
      'f"',
      repeat(choice(
        $._format_string_content,
        $.escape_sequence,
        $.interpolation,
      )),
      token.immediate('"'),
    ),

    interpolation: $ => seq(
      '{',
      $._expression,
      '}',
    ),

    backtick_literal: _ => /`[^`\n]*`/,

    char_literal: _ => token(seq(
      '\'',
      optional(choice(
        seq('\\', ESCAPE_SEQ),
        /[^\\']/,
      )),
      '\'',
    )),

    escape_sequence: _ => token.immediate(seq('\\', ESCAPE_SEQ)),

    boolean_literal: _ => choice('true', 'false'),

    // Comments

    line_comment: _ => token(seq('//', /.*/)),

    doc_comment: _ => token(seq('///', /.*/)),

    // Identifiers

    // @ts-ignore — tree-sitter handles Unicode properties natively
    identifier: _ => /[_\p{XID_Start}][_\p{XID_Continue}]*/,

    self: _ => 'self',

    _type_identifier: $ => alias($.identifier, $.type_identifier),
    _field_identifier: $ => alias($.identifier, $.field_identifier),
  },
});
