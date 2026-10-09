//! Closure-free checks for constant evaluation.

use quote::ToTokens;
use syn::{Block, Expr, Signature, Stmt, parse_quote, visit_mut::VisitMut};

use crate::{
    FnSpec,
    instrument::{CheckSettings, patterns::TamePat},
};

/// Instrument a const body without calling runtime evaluator helpers.
///
/// # Specification
/// - ensures: enabled predicates run at their entry or exit boundary; disabled
///   predicates remain type-checked without evaluation.
/// - ensures: output patterns preserve ownership of the returned value.
/// - fails: rejects printing, deferred panics, captures, and type-spec markers.
/// - panics: none for a parsed function and specification.
///
/// # Adequacy
/// - hypothesis: const-item evaluation and runtime rejection distinguish omitted
///   checks, accidental evaluator calls, and output loss on early returns.
/// - witness: `const_fns::const_items_and_runtime_agree`
/// - witness: `const_fns::violations_reach_the_correct_boundary`
pub(super) fn instrument(
    settings: &CheckSettings,
    spec: &FnSpec,
    sig: &Signature,
    body: &mut Block,
) -> syn::Result<()> {
    if settings.does_print {
        return Err(syn::Error::new_spanned(
            sig.constness,
            "`anodized_print` is not supported on `const fn`; use `anodized_panic` instead",
        ));
    }
    if settings
        .does_panic
        .as_ref()
        .is_some_and(|panic| panic.has_try_fn)
    {
        return Err(syn::Error::new_spanned(
            sig.constness,
            "`anodized_try` is not supported on `const fn`",
        ));
    }
    if spec.output_spec_flag
        || spec
            .input_spec_flags
            .iter()
            .any(|flag| flag.on_entry().is_some() || flag.on_exit().is_some())
    {
        return Err(spec.spec_err("type-spec markers are not supported on `const fn`"));
    }

    let mut statements = Vec::new();
    for condition in spec.requires.iter().chain(&spec.maintains) {
        statements.push(check(
            settings,
            &condition.expr,
            &condition.cfg,
            "precondition",
        ));
    }

    // A labelled block keeps explicit returns inside the checked exit boundary.
    let label = syn::Lifetime::new("'__anodized_body", proc_macro2::Span::mixed_site());
    let mut returns = ConstReturns {
        label: &label,
        found: false,
    };
    returns.visit_block_mut(body);
    let output: Expr = if returns.found {
        parse_quote! { #label: #body }
    } else {
        parse_quote! { #body }
    };
    let output_type = match &sig.output {
        syn::ReturnType::Default => parse_quote! { () },
        syn::ReturnType::Type(_, ty) => *ty.clone(),
    };
    statements.push(parse_quote! {
        let __anodized_output: #output_type = #output;
    });

    for condition in &spec.maintains {
        statements.push(check(
            settings,
            &condition.expr,
            &condition.cfg,
            "postcondition",
        ));
    }
    // Recover the value directly: a temporary (bool, T) would require const
    // destruction when T has drop glue, even though both fields are moved out.
    for condition in &spec.ensures {
        let check = check(settings, &condition.expr, &condition.cfg, "postcondition");
        match &condition.pat {
            Some(TamePat::Invertible(pattern, inverse)) => statements.push(parse_quote! {
                let __anodized_output = {
                    let #pattern = __anodized_output;
                    #check
                    #inverse
                };
            }),
            Some(TamePat::Borrowing(pattern)) => statements.push(parse_quote! {
                let __anodized_output = {
                    let #pattern = __anodized_output;
                    #check
                    __anodized_output
                };
            }),
            None => statements.push(check),
        }
    }
    statements.push(Stmt::Expr(parse_quote! { __anodized_output }, None));
    body.stmts = statements;
    Ok(())
}

/// Build a const-compatible predicate check with a static panic message.
///
/// # Specification
/// - ensures: only enabled checks evaluate their Boolean expression.
/// - ensures: disabled expressions remain type-checked.
/// - panics: none for parsed expressions.
fn check(
    settings: &CheckSettings,
    expression: &Expr,
    cfg: &Option<syn::Meta>,
    boundary: &str,
) -> Stmt {
    if settings.does_panic.is_none() {
        return parse_quote! { if false { let _: bool = #expression; } };
    }
    let message = format!("{boundary} failed: {}", expression.to_token_stream());
    let guard = cfg.as_ref().map(|meta| quote::quote! { cfg!(#meta) && });
    parse_quote! {
        if #guard !(#expression) { panic!("{}", #message); }
    }
}

/// Redirect explicit returns without entering independently returning scopes.
struct ConstReturns<'a> {
    /// Hygienic target shared by rewritten returns and the body block.
    label: &'a syn::Lifetime,
    /// Whether the body needs a labelled exit.
    found: bool,
}

impl VisitMut for ConstReturns<'_> {
    /// Rewrite a return after visiting its operand.
    ///
    /// # Specification
    /// - ensures: explicit function returns exit the checked body block.
    /// - panics: none for a parsed expression.
    fn visit_expr_mut(&mut self, expression: &mut Expr) {
        syn::visit_mut::visit_expr_mut(self, expression);
        if let Expr::Return(returned) = expression {
            self.found = true;
            let label = self.label;
            let value = &returned.expr;
            let attrs = &returned.attrs;
            *expression = parse_quote! { #(#attrs)* break #label #value };
        }
    }

    /// Leave nested closures' returns alone.
    ///
    /// # Specification
    /// trivial.
    fn visit_expr_closure_mut(&mut self, _: &mut syn::ExprClosure) {}

    /// Leave inline constant scopes alone.
    ///
    /// # Specification
    /// trivial.
    fn visit_expr_const_mut(&mut self, _: &mut syn::ExprConst) {}

    /// Leave nested item bodies alone.
    ///
    /// # Specification
    /// trivial.
    fn visit_item_mut(&mut self, _: &mut syn::Item) {}
}
