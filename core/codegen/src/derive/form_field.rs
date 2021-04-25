use devise::{*, ext::{TypeExt, SpanDiagnosticExt}};

use syn::visit_mut::VisitMut;
use syn::visit::Visit;

use crate::proc_macro2::{TokenStream, TokenTree, Span};
use crate::syn_ext::IdentExt;
use crate::name::Name;

#[derive(Debug)]
pub enum FieldName {
    Cased(Name),
    Uncased(Name),
}

#[derive(FromMeta)]
pub struct FieldAttr {
    pub name: Option<FieldName>,
    pub validate: Option<SpanWrapped<syn::Expr>>,
}

impl FieldAttr {
    const NAME: &'static str = "field";
}

pub(crate) trait FieldExt {
    fn ident(&self) -> &syn::Ident;
    fn field_names(&self) -> Result<Vec<FieldName>>;
    fn first_field_name(&self) -> Result<FieldName>;
    fn stripped_ty(&self) -> syn::Type;
    fn name_view(&self) -> Result<syn::Expr>;
}

#[derive(FromMeta)]
pub struct VariantAttr {
    pub value: Name,
}

impl VariantAttr {
    const NAME: &'static str = "field";
}

pub(crate) trait VariantExt {
    fn first_form_field_value(&self) -> Result<FieldName>;
    fn form_field_values(&self) -> Result<Vec<FieldName>>;
}

impl VariantExt for Variant<'_> {
    fn first_form_field_value(&self) -> Result<FieldName> {
        let first = VariantAttr::from_attrs(VariantAttr::NAME, &self.attrs)?
            .into_iter()
            .next();

        Ok(first.map_or_else(
                || FieldName::Uncased(Name::from(&self.ident)),
                |attr| FieldName::Uncased(attr.value)))
    }

    fn form_field_values(&self) -> Result<Vec<FieldName>> {
        let attr_values = VariantAttr::from_attrs(VariantAttr::NAME, &self.attrs)?
            .into_iter()
            .map(|attr| FieldName::Uncased(attr.value))
            .collect::<Vec<_>>();

        if attr_values.is_empty() {
            return Ok(vec![FieldName::Uncased(Name::from(&self.ident))]);
        }

        Ok(attr_values)
    }
}

impl FromMeta for FieldName {
    fn from_meta(meta: &MetaItem) -> Result<Self> {
        // These are used during parsing.
        const CONTROL_CHARS: &[char] = &['&', '=', '?', '.', '[', ']'];

        fn is_valid_field_name(s: &str) -> bool {
            // The HTML5 spec (4.10.18.1) says 'isindex' is not allowed.
            if s == "isindex" || s.is_empty() {
                return false
            }

            // We allow all visible ASCII characters except `CONTROL_CHARS`.
            s.chars().all(|c| c.is_ascii_graphic() && !CONTROL_CHARS.contains(&c))
        }

        let field_name = match Name::from_meta(meta) {
            Ok(name) => FieldName::Cased(name),
            Err(_) => {
                #[derive(FromMeta)]
                struct Inner {
                    #[meta(naked)]
                    uncased: Name
                }

                let expr = meta.expr()?;
                let item: MetaItem = syn::parse2(quote!(#expr))?;
                let inner = Inner::from_meta(&item)?;
                FieldName::Uncased(inner.uncased)
            }
        };

        if !is_valid_field_name(field_name.as_str()) {
            let chars = CONTROL_CHARS.iter()
                .map(|c| format!("{:?}", c))
                .collect::<Vec<_>>()
                .join(", ");

            return Err(meta.value_span()
                .error("invalid form field name")
                .help(format!("field name cannot be `isindex` or contain {}", chars)));
        }

        Ok(field_name)
    }
}

impl std::ops::Deref for FieldName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        match self {
            FieldName::Cased(n) | FieldName::Uncased(n) => n,
        }
    }
}

impl quote::ToTokens for FieldName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        (self as &Name).to_tokens(tokens)
    }
}

impl PartialEq for FieldName {
    fn eq(&self, other: &Self) -> bool {
        use FieldName::*;

        match (self, other) {
            (Cased(a), Cased(b)) => a == b,
            (Cased(a), Uncased(u)) | (Uncased(u), Cased(a)) => a == u.as_uncased_str(),
            (Uncased(u1), Uncased(u2)) => u1.as_uncased_str() == u2.as_uncased_str(),
        }
    }
}

impl FieldExt for Field<'_> {
    fn ident(&self) -> &syn::Ident {
        self.ident.as_ref().expect("named")
    }

    fn field_names(&self) -> Result<Vec<FieldName>> {
        let attr_names = FieldAttr::from_attrs(FieldAttr::NAME, &self.attrs)?
            .into_iter()
            .filter_map(|attr| attr.name)
            .collect::<Vec<_>>();

        if attr_names.is_empty() {
            let ident_name = Name::from(self.ident());
            return Ok(vec![FieldName::Cased(ident_name)]);
        }

        Ok(attr_names)
    }

    fn first_field_name(&self) -> Result<FieldName> {
        let mut names = self.field_names()?.into_iter();
        Ok(names.next().expect("always have >= 1 name"))
    }

    fn stripped_ty(&self) -> syn::Type {
        self.ty.with_stripped_lifetimes()
    }

    fn name_view(&self) -> Result<syn::Expr> {
        let field_names = self.field_names()?;
        let field_name = field_names.first().expect("always have name");
        define_spanned_export!(self.span() => _form);
        let name_view = quote_spanned! { self.span() =>
            #_form::NameBuf::from((__c.__parent, #field_name))
        };

        Ok(syn::parse2(name_view).unwrap())
    }
}

struct RecordMemberAccesses(Vec<syn::Member>);

impl<'a> Visit<'a> for RecordMemberAccesses {
    fn visit_expr_field(&mut self, i: &syn::ExprField) {
        if let syn::Expr::Path(e) = &*i.base {
            if e.path.is_ident("self") {
                self.0.push(i.member.clone());
            }
        }

        syn::visit::visit_expr_field(self, i);
    }
}

struct ValidationMutator<'a> {
    field: &'a syn::Ident,
    parent: &'a syn::Ident,
    local: bool,
    visited: bool,
}

impl ValidationMutator<'_> {
    fn visit_token_stream(&mut self, tt: TokenStream) -> TokenStream {
        use quote::{ToTokens, TokenStreamExt};
        use TokenTree::*;

        let mut iter = tt.into_iter();
        let mut stream = TokenStream::new();
        while let Some(tt) = iter.next() {
            match tt {
                Ident(s3lf) if s3lf == "self" => {
                    match (iter.next(), iter.next()) {
                        (Some(Punct(p)), Some(Ident(i))) if p.as_char() == '.' => {
                            let field = syn::parse_quote!(#s3lf #p #i);
                            let mut expr = syn::Expr::Field(field);
                            self.visit_expr_mut(&mut expr);
                            expr.to_tokens(&mut stream);
                        },
                        (tt1, tt2) => stream.append_all(&[Some(Ident(s3lf)), tt1, tt2]),
                    }
                },
                TokenTree::Group(group) => {
                    let tt = self.visit_token_stream(group.stream());
                    let mut new = proc_macro2::Group::new(group.delimiter(), tt);
                    new.set_span(group.span());
                    let group = TokenTree::Group(new);
                    stream.append(group);
                }
                tt => stream.append(tt),
            }
        }

        stream
    }
}

impl VisitMut for ValidationMutator<'_> {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);

        // Only modify the first call we see.
        if self.visited { return; }

        let (parent, field) = (self.parent, self.field);
        let form_field = match self.local {
            true => syn::parse2(quote_spanned!(field.span() => &#field)).unwrap(),
            false => syn::parse2(quote_spanned!(field.span() => &#parent.#field)).unwrap(),
        };

        call.args.insert(0, form_field);
        self.visited = true;
    }

    fn visit_ident_mut(&mut self, i: &mut syn::Ident) {
        if !self.local && i == "self" {
            *i = self.parent.clone();
        }
    }

    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
        mac.tokens = self.visit_token_stream(mac.tokens.clone());
        syn::visit_mut::visit_macro_mut(self, mac);
    }

    fn visit_expr_mut(&mut self, i: &mut syn::Expr) {
        // If this is a local, replace accesses of `self.field` with `field`.
        if let syn::Expr::Field(e) = i {
            if let syn::Expr::Path(e) = &*e.base {
                if e.path.is_ident("self") && self.local {
                    let new_expr = &self.field;
                    *i = syn::parse_quote!(#new_expr);
                }
            }
        }

        return syn::visit_mut::visit_expr_mut(self, i);
    }
}

pub fn validators<'v>(
    field: Field<'v>,
    parent: &'v syn::Ident, // field ident (if local) or form ident (if !local)
    local: bool,
) -> Result<impl Iterator<Item = syn::Expr> + 'v> {
    let exprs = FieldAttr::from_attrs(FieldAttr::NAME, &field.attrs)?
        .into_iter()
        .filter_map(|a| a.validate)
        .map(move |expr| {
            let mut members = RecordMemberAccesses(vec![]);
            members.visit_expr(&expr);

            let field_ident = field.ident();
            let is_local_validation = members.0.iter()
                .all(|member| match member {
                    syn::Member::Named(i) => i == field_ident,
                    _ => false
                });

            (expr, is_local_validation)
        })
        .filter(move |(_, is_local)| *is_local == local)
        .map(move |(mut expr, _)| {
            let field_span = field.ident().span()
                .join(field.ty.span())
                .unwrap_or(field.ty.span());

            let field_ident = field.ident().clone().with_span(field_span);
            let mut v = ValidationMutator { parent, local, field: &field_ident, visited: false };
            v.visit_expr_mut(&mut expr);

            let span = expr.key_span.unwrap_or(field_span);
            define_spanned_export!(span => _form);
            syn::parse2(quote_spanned!(span => {
                let __result: #_form::Result<'_, ()> = #expr;
                __result
            })).unwrap()
        });

        Ok(exprs)
}

pub fn first_duplicate<K: Spanned, V: PartialEq + Spanned>(
    keys: impl Iterator<Item = K> + Clone,
    values: impl Fn(&K) -> Result<Vec<V>>,
) -> Result<Option<((usize, Span, Span), (usize, Span, Span))>> {
    let (mut all_values, mut key_map) = (vec![], vec![]);
    for key in keys {
        all_values.append(&mut values(&key)?);
        key_map.push((all_values.len(), key));
    }

    // get the key corresponding to all_value index `k`.
    let key = |k| key_map.iter().find(|(i, _)| k < *i).expect("k < *i");

    for (i, a) in all_values.iter().enumerate() {
        let rest = all_values.iter().enumerate().skip(i + 1);
        if let Some((j, b)) = rest.filter(|(_, b)| *b == a).next() {
            let (a_i, key_a) = key(i);
            let (b_i, key_b) = key(j);

            let a = (*a_i, key_a.span(), a.span());
            let b = (*b_i, key_b.span(), b.span());
            return Ok(Some((a, b)));
        }
    }

    Ok(None)
}
