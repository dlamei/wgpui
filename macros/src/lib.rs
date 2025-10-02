use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, DeriveInput, Ident, ItemStruct, LitInt, Token, Type};
use syn::parse::{Parse, ParseStream, Result};



#[proc_macro_attribute]
pub fn vertex(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemStruct);
    let name = &input.ident;

    // Only allow named-field structs
    let fields = match &input.fields {
        syn::Fields::Named(named) => named.named.iter().collect::<Vec<_>>(),
        _ => panic!("#[vertex] can only be used on structs with named fields"),
    };

    // Compute field offsets
    let mut offset_exprs = Vec::new();
    let mut current_offset = quote! { 0usize };

    for (i, field) in fields.iter().enumerate() {
        offset_exprs.push(current_offset.clone());
        if i < fields.len() - 1 {
            let ty = &field.ty;
            current_offset = quote! {
                #current_offset + ::std::mem::size_of::<#ty>()
            };
        }
    }

    // Build VertexAttribute array
    let attributes = fields.iter().zip(offset_exprs.clone()).enumerate().map(|(i, (f, offset))| {
        let ty = &f.ty;
        let location = i as u32;
        quote! {
            wgpu::VertexAttribute {
                offset: (#offset) as u64,
                shader_location: #location,
                format: <#ty as wgpui::AsVertexFormat>::VERTEX_FORMAT,
            }
        }
    });

    let member_names = fields.iter().map(|f| {
        f.ident.as_ref().unwrap().to_string()
    });

    // CamelCase -> snake_case
    let label = name.to_string()
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_uppercase() && i != 0 {
                format!("_{}", c.to_ascii_lowercase())
            } else {
                c.to_ascii_lowercase().to_string()
            }
        })
        .collect::<String>();

    let expanded = quote! {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
        #input

        impl wgpui::Vertex for #name {
            const VERTEX_LABEL: &'static str = #label;
            const VERTEX_ATTRIBUTES: &'static [wgpu::VertexAttribute] = &[
                #(#attributes, )*
            ];
            const VERTEX_MEMBERS: &'static [&'static str] = &[
                #(#member_names, )*
            ];
        }
    };

    TokenStream::from(expanded)
}


#[proc_macro_attribute]
pub fn wgsl(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse attribute argument as identifier
    let wgsl_ident = parse_macro_input!(attr as Ident);
    let wgsl_name_str = wgsl_ident.to_string();

    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let fields = match &input.fields {
        syn::Fields::Named(named) => named.named.iter().collect::<Vec<_>>(),
        _ => panic!("#[wgsl] can only be used on structs with named fields"),
    };

    // compute offsets
    let mut offset_exprs = Vec::new();
    let mut current_offset = quote! { 0usize };

    for (i, field) in fields.iter().enumerate() {
        offset_exprs.push(current_offset.clone());
        if i < fields.len() - 1 {
            let ty = &field.ty;
            current_offset = quote! {
                #current_offset + ::std::mem::size_of::<#ty>()
            };
        }
    }

    // build VertexAttribute array
    let attributes = fields.iter().zip(offset_exprs.clone()).enumerate().map(|(i, (f, offset))| {
        let ty = &f.ty;
        let location = i as u32;
        quote! {
            wgpu::VertexAttribute {
                offset: (#offset) as u64,
                shader_location: #location,
                format: <#ty as wgpui::AsVertexFormat>::VERTEX_FORMAT,
            }
        }
    });

    let member_names = fields.iter().map(|f| {
        f.ident.as_ref().unwrap().to_string()
    });

    // CamelCase -> snake_case label
    let label = name.to_string()
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_uppercase() && i != 0 {
                format!("_{}", c.to_ascii_lowercase())
            } else {
                c.to_ascii_lowercase().to_string()
            }
        })
        .collect::<String>();

    let expanded = quote! {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
        #input

        impl wgpui::Vertex for #name {
            const VERTEX_LABEL: &'static str = #label;
            const VERTEX_ATTRIBUTES: &'static [wgpu::VertexAttribute] = &[
                #(#attributes, )*
            ];
            const VERTEX_MEMBERS: &'static [&'static str] = &[
                #(#member_names, )*
            ];
            const WGSL_NAME: &'static str = #wgsl_name_str;
        }
    };

    TokenStream::from(expanded)
}





const LOREM_WORDS: &[&str] = &[
    "lorem","ipsum","dolor","sit","amet","consectetur","adipiscing","elit","sed","do","eiusmod",
    "tempor","incididunt","ut","labore","et","dolore","magna","aliqua","enim","ad","minim",
    "veniam","quis","nostrud","exercitation","ullamco","laboris","nisi","aliquip","ex","ea",
    "commodo","consequat","duis","aute","irure","in","reprehenderit","voluptate","velit","esse",
    "cillum","eu","fugiat","nulla","pariatur","excepteur","sint","occaecat","cupidatat","non",
    "proident","sunt","culpa","qui","officia","deserunt","mollit","anim","id","est","laborum"
];


struct LoremArgs {
    words: u32,
    sentences: u32,
    paragraphs: u32,
}

impl Parse for LoremArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut args = LoremArgs { words: 5, sentences: 0, paragraphs: 1 };

        let pairs = Punctuated::<syn::Expr, Token![,]>::parse_terminated(input)?;
        for expr in pairs {
            if let syn::Expr::Assign(assign) = expr {
                if let (syn::Expr::Path(left), syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(v), .. })) =
                    (*assign.left, *assign.right)
                {
                    if let Some(id) = left.path.get_ident() {
                        match id.to_string().as_str() {
                            "words" => args.words = v.base10_parse::<u32>()?,
                            "sentences" => args.sentences = v.base10_parse::<u32>()?,
                            "paragraphs" => args.paragraphs = v.base10_parse::<u32>()?,
                            _ => (),
                        }
                    }
                }
            }
        }
        Ok(args)
    }
}


#[proc_macro]
pub fn lorem(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as LoremArgs);

    // simple xorshift-like LCG RNG (no external deps)
    fn next_rand(rng: &mut u64) -> u64 {
        *rng = rng.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64);
        *rng
    }
    fn rand_signed(rng: &mut u64, range: i32) -> i32 {
        // returns value in [-range, range]
        let r = (next_rand(rng) & 0xFFFF) as i32;
        let span = range * 2 + 1;
        (r % span) - range
    }
    fn rand_range(rng: &mut u64, min: u32, max: u32) -> u32 {
        if max <= min { return min; }
        let span = max - min + 1;
        let v = (next_rand(rng) >> 16) as u32;
        min + (v % span)
    }

    let mut result = String::new();
    let mut idx = 0usize;

    // seed based on args to make output vary by args but deterministic per compilation
    let mut rng: u64 = (args.words as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (args.sentences as u64).wrapping_mul(0xC6A4A7935BD1E995)
        ^ (args.paragraphs as u64).wrapping_add(0x12345678);

    for p in 0..args.paragraphs {
        if p > 0 {
            result.push_str("\n\n");
        }

        if args.sentences == 0 {
            // produce a single run of words with no period; words = avg words per "sentence"
            // add small variation +/-1
            let variation = rand_signed(&mut rng, 1); // -1..1
            let word_count = ((args.words as i32 + variation).max(1)) as u32;
            for w in 0..word_count {
                if w > 0 { result.push(' '); }
                let word = LOREM_WORDS[idx % LOREM_WORDS.len()];
                if w == 0 {
                    // capitalize first word of paragraph
                    let mut chs = word.chars();
                    if let Some(first) = chs.next() {
                        result.push_str(&first.to_uppercase().to_string());
                        result.push_str(chs.as_str());
                    } else {
                        result.push_str(word);
                    }
                } else {
                    result.push_str(word);
                }
                idx = idx.wrapping_add(1);
            }
            // no trailing period
            continue;
        }

        // sentences > 0: for this paragraph choose sentence count around average (±1)
        let sent_variation = rand_signed(&mut rng, 1); // -1..1
        let sentences_in_par = ((args.sentences as i32 + sent_variation).max(1)) as u32;

        for s in 0..sentences_in_par {
            if s > 0 {
                result.push(' ');
            }
            // words per sentence around average (±1 or ±2 depending on size)
            let word_var_range = if args.words >= 6 { 2 } else { 1 };
            let wvar = rand_signed(&mut rng, word_var_range);
            let words_in_sent = ((args.words as i32 + wvar).max(1)) as u32;

            for w in 0..words_in_sent {
                if w > 0 {
                    result.push(' ');
                }
                let word = LOREM_WORDS[idx % LOREM_WORDS.len()];
                if w == 0 {
                    // capitalize first word of sentence
                    let mut chs = word.chars();
                    if let Some(first) = chs.next() {
                        result.push_str(&first.to_uppercase().to_string());
                        result.push_str(chs.as_str());
                    } else {
                        result.push_str(word);
                    }
                } else {
                    result.push_str(word);
                }
                idx = idx.wrapping_add(1);
            }
            result.push('.');
        }
    }

    let lit = syn::LitStr::new(&result, proc_macro2::Span::call_site());
    TokenStream::from(quote! { #lit })
}



// #[proc_macro]
// pub fn lorem(input: TokenStream) -> TokenStream {
//     let args = parse_macro_input!(input as LoremArgs);

//     let mut result = String::new();
//     let mut idx = 0;

//     for p in 0..args.paragraphs {
//         if p > 0 {
//             result.push_str("\n\n");
//         }
//         for s in 0..args.sentences {
//             if s > 0 {
//                 result.push(' ');
//             }
//             let len = args.words / args.sentences;
//             for w in 0..len {
//                 let word = LOREM_WORDS[idx % LOREM_WORDS.len()];
//                 if w == 0 {
//                     result.push_str(&word[..1].to_uppercase());
//                     result.push_str(&word[1..]);
//                 } else {
//                     result.push(' ');
//                     result.push_str(word);
//                 }
//                 idx += 1;
//             }
//             result.push('.');
//         }
//     }

//     let lit = syn::LitStr::new(&result, proc_macro2::Span::call_site());
//     TokenStream::from(quote! { #lit })
// }


// struct FlagsInput {
//     ty: Ident,
//     flags: Punctuated<Ident, Token![,]>,
// }

// impl syn::parse::Parse for FlagsInput {
//     fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
//         let ty: Ident = input.parse()?;
//         input.parse::<Token![:]>()?;
//         let flags = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
//         Ok(FlagsInput { ty, flags })
//     }
// }

// #[proc_macro]
// pub fn flags(input: TokenStream) -> TokenStream {
//     let FlagsInput { ty, flags } = parse_macro_input!(input as FlagsInput);

//     let mut consts = Vec::new();
//     for (i, flag) in flags.iter().enumerate() {
//         let shift = i as u32;
//         consts.push(quote! {
//             const #flag = 1 << #shift;
//         });
//     }

//     let expanded = quote! {
//         bitflags::bitflags! {
//             #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
//             pub struct #ty: u32 {
//                 const NONE = 0;
//                 #(#consts)*
//             }
//         }

//         impl #ty {
//             pub fn has(self, other: #ty) -> bool {
//                 self.contains(other)
//             }
//         }
//     };

//     TokenStream::from(expanded)
// }

enum FlagItem {
    Auto(Ident),
    OrAssign(Ident, syn::Expr),
    Assign(Ident, syn::Expr),
}

impl Parse for FlagItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        if input.peek(Token![|]) && input.peek2(Token![=]) {
            input.parse::<Token![|]>()?;
            input.parse::<Token![=]>()?;
            let expr: syn::Expr = input.parse()?;
            Ok(FlagItem::OrAssign(name, expr))
        } else if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let expr: syn::Expr = input.parse()?;
            Ok(FlagItem::Assign(name, expr))
        } else {
            Ok(FlagItem::Auto(name))
        }
    }
}

struct FlagsInput {
    ty: Ident,
    flags: Punctuated<FlagItem, Token![,]>,
}

impl Parse for FlagsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ty: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let flags = Punctuated::<FlagItem, Token![,]>::parse_terminated(input)?;
        Ok(FlagsInput { ty, flags })
    }
}

#[proc_macro]
pub fn flags(input: TokenStream) -> TokenStream {
    let FlagsInput { ty, flags } = parse_macro_input!(input as FlagsInput);

    let mut consts = Vec::new();
    for (i, flag_item) in flags.iter().enumerate() {
        let shift = i as u32;
        match flag_item {
            FlagItem::Auto(ident) => {
                consts.push(quote! {
                    const #ident = 1 << #shift;
                });
            }
            FlagItem::OrAssign(ident, expr) => {
                consts.push(quote! {
                    const #ident = 1 << #shift | Self::#expr.bits();
                });
            }
            FlagItem::Assign(ident, expr) => {
                consts.push(quote! {
                    const #ident = #expr;
                });
            }
        }
    }

    let expanded = quote! {
        bitflags::bitflags! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub struct #ty: u32 {
                const NONE = 0;
                #(#consts)*
            }
        }

        impl #ty {
            pub fn has(self, other: #ty) -> bool {
                self.contains(other)
            }
        }
    };

    TokenStream::from(expanded)
}







