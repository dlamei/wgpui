use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Token, Ident, Type, LitInt};
use syn::parse::{Parse, ParseStream, Result};

fn camel_to_snake(camel: &str) -> String {
    let mut snake = String::new();

    let mut prev_upper = false;
    let mut first = true;
    for c in camel.chars() {
        if c.is_uppercase() && !prev_upper && !first{
            snake += "_";
            prev_upper = true;
        } else if c.is_lowercase() {
            prev_upper = false;
        }
        snake += &c.to_lowercase().to_string();
        first = false;
    }

    snake
}

struct VertexField {
    name: Ident,
    ty: Type,
    location: u32,
}

struct VertexStruct {
    name: Ident,
    fields: Vec<VertexField>,
}

impl Parse for VertexStruct {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        
        let content;
        syn::braced!(content in input);
        
        let mut fields = Vec::new();
        
        while !content.is_empty() {
            let field_name: Ident = content.parse()?;
            
            // Parse parentheses with location
            let location_content;
            syn::parenthesized!(location_content in content);
            let location: LitInt = location_content.parse()?;
            
            // Parse colon
            content.parse::<Token![:]>()?;
            
            // Parse type
            let field_type: Type = content.parse()?;
            
            fields.push(VertexField {
                name: field_name,
                ty: field_type,
                location: location.base10_parse()?,
            });
            
            // Parse optional comma
            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }
        }
        
        Ok(VertexStruct { name, fields })
    }
}

#[proc_macro]
pub fn vertex_struct(input: TokenStream) -> TokenStream {
    let vertex_struct = parse_macro_input!(input as VertexStruct);
    
    let name = &vertex_struct.name;
    let field_defs = vertex_struct.fields.iter().map(|f| {
        let field_name = &f.name;
        let field_type = &f.ty;
        quote! { pub #field_name: #field_type }
    });
    
    // Calculate offsets manually
    let mut offset_exprs = Vec::new();
    let mut current_offset = quote! { 0 };
    
    for (i, field) in vertex_struct.fields.iter().enumerate() {
        offset_exprs.push(current_offset.clone());
        
        if i < vertex_struct.fields.len() - 1 {
            let field_type = &field.ty;
            current_offset = quote! {
                #current_offset + std::mem::size_of::<#field_type>()
            };
        }
    }

    let n_attribs = vertex_struct.fields.len();
    
    let attributes = vertex_struct.fields.iter().zip(offset_exprs.clone()).map(|(f, offset)| {
        let location = f.location;
        let ty = f.ty.clone();
        
        quote! {
            wgpu::VertexAttribute {
                offset: (#offset) as u64,
                shader_location: #location,
                format: <#ty as wgpui::AsVertexFormat>::VERTEX_FORMAT,
            }
        }
    });

    let member_names = vertex_struct.fields.iter().map(|f| {
        f.name.to_string()
    });

    // let attributes_offset = vertex_struct.fields.iter().zip(offset_exprs).map(|(f, offset)| {
    //     let location = f.location;
    //     let ty = f.ty.clone();
        
    //     quote! {
    //         wgpu::VertexAttribute {
    //             offset: (#offset) as u64,
    //             shader_location: #location + offset,
    //             format: <#ty as wgpui::AsVertexFormat>::FORMAT,
    //         }
    //     }
    // });

    let label = camel_to_snake(&name.to_string());
    
    let expanded = quote! {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        pub struct #name {
            #(#field_defs,)*
        }

        impl wgpui::Vertex for #name {
            const VERTEX_LABEL: &'static str = #label;
            const VERTEX_ATTRIBUTES: &'static [wgpu::VertexAttribute] = &[
                #(#attributes,)*
            ];

            const VERTEX_MEMBERS: &'static [&'static str] = &[
                #(#member_names,)*
            ];
        }
        
        // impl #name {

        //     pub const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; #n_attribs] = [
        //         #(#attributes,)*
        //     ];

        //     pub fn vertex_attributes_offset(offset: u32) -> [wgpu::VertexAttribute; #n_attribs] {
        //         [
        //             #(#attributes_offset,)*
        //         ]
        //     }

        //     pub fn buffer_layout_with_attributes<'a>(attribs: &'a [wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'a> {
        //         wgpu::VertexBufferLayout {
        //             array_stride: std::mem::size_of::<#name>() as wgpu::BufferAddress,
        //             step_mode: wgpu::VertexStepMode::Vertex,
        //             attributes: attribs,
        //         }
        //     }
            
        //     pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        //         Self::buffer_layout_with_attributes(&Self::VERTEX_ATTRIBUTES)
        //     }

        // }
    };
    
    TokenStream::from(expanded)
}

// struct VertexField {
//     name: Ident,
//     ty: Type,
//     location: u32,
// }

// struct VertexStruct {
//     name: Ident,
//     fields: Vec<VertexField>,
// }

// impl Parse for VertexStruct {
//     fn parse(input: ParseStream) -> Result<Self> {
//         let name: Ident = input.parse()?;
        
//         let content;
//         syn::braced!(content in input);
        
//         let mut fields = Vec::new();
        
//         while !content.is_empty() {
//             let field_name: Ident = content.parse()?;
//             content.parse::<Token![:]>()?;
//             let field_type: Type = content.parse()?;
//             content.parse::<Token![=]>()?;
//             let location: LitInt = content.parse()?;
            
//             fields.push(VertexField {
//                 name: field_name,
//                 ty: field_type,
//                 location: location.base10_parse()?,
//             });
            
//             if !content.is_empty() {
//                 content.parse::<Token![,]>()?;
//             }
//         }
        
//         Ok(VertexStruct { name, fields })
//     }
// }

// #[proc_macro]
// pub fn vertex_struct(input: TokenStream) -> TokenStream {
//     let vertex_struct = parse_macro_input!(input as VertexStruct);
    
//     let name = &vertex_struct.name;
//     let field_defs = vertex_struct.fields.iter().map(|f| {
//         let field_name = &f.name;
//         let field_type = &f.ty;
//         quote! { pub #field_name: #field_type }
//     });
    
//     // Calculate offsets manually
//     let mut offset_exprs = Vec::new();
//     let mut current_offset = quote! { 0 };
    
//     for (i, field) in vertex_struct.fields.iter().enumerate() {
//         offset_exprs.push(current_offset.clone());
        
//         if i < vertex_struct.fields.len() - 1 {
//             let field_type = &field.ty;
//             current_offset = quote! {
//                 #current_offset + std::mem::size_of::<#field_type>()
//             };
//         }
//     }
    
//     let attributes = vertex_struct.fields.iter().zip(offset_exprs).map(|(f, offset)| {
//         let location = f.location;
//         // let format = get_vertex_format(&f.ty);

//         let ty = f.ty.clone();
        
//         quote! {
//             wgpu::VertexAttribute {
//                 offset: (#offset) as u64,
//                 shader_location: #location,
//                 format: <#ty as wgpui::AsVertexFormat>::FORMAT,
//             }
//         }
//     });
    
//     let expanded = quote! {
//         #[repr(C)]
//         #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
//         pub struct #name {
//             #(#field_defs,)*
//         }
        
//         impl #name {
//             pub fn attributes() -> &'static [wgpu::VertexAttribute] {
//                 &[
//                     #(#attributes,)*
//                 ]
//             }
            
//             pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
//                 wgpu::VertexBufferLayout {
//                     array_stride: std::mem::size_of::<#name>() as wgpu::BufferAddress,
//                     step_mode: wgpu::VertexStepMode::Vertex,
//                     attributes: Self::attributes(),
//                 }
//             }
//         }
//     };

//     TokenStream::from(expanded)
// }

