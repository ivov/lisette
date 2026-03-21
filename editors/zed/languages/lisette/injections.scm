; Inject Go into @rawgo directive string arguments
(rawgo_directive
  (string_literal
    (string_content) @injection.content)
  (#set! injection.language "go"))
