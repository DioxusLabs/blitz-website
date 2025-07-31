use dioxus::prelude::*;

use crate::components::AnchorHeader;
use crate::components::Page;
use crate::components::SectionLevel::*;

#[component]
pub fn AboutPage() -> Element {
    rsx! {
        Page {
            title: "About".into(),

            h1 { "About" }

            p {
                dangerous_inner_html: r#"
                    Blitz is an open source web engine written in Rust with a focus on modularity, embeddibilty, and API flexibility.
                    Blitz is currently in <b>alpha</b>. This means that it ready for experimentation and early adoption, but is not ready for production
                    usage. We are aiming to reach a broadly usable <b>beta</b> status by the end of 2025, with a production-ready release sometime in 2026.
                "#
            }
            p {
                dangerous_inner_html: r#"
                    For more details on the current status, see the dedicated <a href="/status">status page</a>.<br/ >
                    For details on planned work, see the <a href="https://github.com/DioxusLabs/blitz/issues/119">roadmap issue</a>
                "#
            }

            AnchorHeader {
                level: H2,
                target: "vision",
                "Vision"
            }

            p {
                "By taking a modular approach to development with an emphasis on clean module boundaries and sharing code, Blitz aims to:"
            }

            ol {
                li {
                    dangerous_inner_html: r#"
                        <b>Power <a href="https://github.com/DioxusLabs/dioxus/tree/main/packages/native" target="_blank">Dioxus Native</a></b> (and perhaps other UI toolkits) - ...
                    "#
                }
                li {
                    p {
                        dangerous_inner_html: r#"
                            <b>Enable "alternative" web engine use cases</b>. Including:
                        "#
                    }
                    // p {
                    //     "Alternative use cases may include:"
                    // }
                    ul {
                        style: "margin-bottom: 12px;",
                        li {
                            style: "margin-bottom: 6px",
                            dangerous_inner_html: r#"
                               Rendering ePUB, HTML Email, Markdown and other non-web HTML formats.<br />
                               <span style="font-size: 0.9em;color: #666">(see the <a href="https://github.com/DioxusLabs/blitz/tree/main/apps/readme">readme app</a> for markdown)</span>
                            "#
                        }
                        li {
                            style: "margin-bottom: 6px",
                            dangerous_inner_html: r#"
                               Rendering HTML to image (PNG, JPEG, or even SVG)<br />
                               <span style="font-size: 0.9em;color: #666">
                                   (see <a href="https://github.com/Jamedjo/himg">himg</a> or the
                                   <a href="https://github.com/DioxusLabs/blitz/blob/main/examples/screenshot.rs">screenshot example</a>
                                   for examples of rendering to image).
                                </span>
                            "#
                        }
                        li {
                            style: "margin-bottom: 6px",
                            dangerous_inner_html: r#"
                               Rendering HTML to PDF
                            "#
                        }
                        li {
                            style: "margin-bottom: 6px",
                            dangerous_inner_html: r#"
                                Embedded “mini html” engines within wider UI toolkits.
                            "#
                        }
                        li {
                            style: "margin-bottom: 6px",
                            dangerous_inner_html: r#"
                                Clients for alternative content (e.g. a Gopher / Gemini) or alternative scripting environments such as
                                a native HTMX client, or a web client with Python-based scripting.
                            "#
                        }
                    }
                }
                li {
                    style: "margin-bottom: 12px",
                    dangerous_inner_html: r#"
                        <b>Provide a new alternative to existing web engines.</b> - In this regard Blitz aims competes with the likes of Servo, Ladybird, and Flow. This is a longer term aim. C
                    "#
                }
                li {
                    p {
                        dangerous_inner_html: r#"
                            <b>Lower the barrier to entry for creating new browser engines</b>
                            — Traditional browser engines are largely monolithic meaning
                            that if you want to create a new engine you are left with a choice between forking the existing engine or rebuilding most
                            things from scratch. Given how enormous the web specification is, this makes it difficult to bootstrap new independent engines.
                        "#
                    }
                     p {
                        dangerous_inner_html: r#"
                            By building on existing general-purpose libraries (making our improvements available to all users) and making core parts of our
                            stack (style, layout, text, etc) available as libraries with supported public APIs, Blitz aims to make it easier to create new
                            engines by enabling new engine creators to combine modular <em>parts</em> of our engine with new modules of their own.
                        "#
                    }
                }
            }

            AnchorHeader {
                level: H2,
                target: "strategy",
                "Strategy"
            }

            p {
                "Blitz is deliberately focussing on (1) and (2) from above and avoiding trying to build an \"entire web browser\" at once."
            }
            p {
                "
                 and focussing on two targeted subsets: 1. being an excellent
                application runtime (for Dioxus Native) and 2. HTML-and-CSS-only rendering. By deferring work on complex features like JavaScript execution,
                browser-grade network caching and security, and process-isolation we hope to be able to bring Blitz to a useful, production-ready state (for the
                features it supports) much sooner. And we hope that "
            }


            AnchorHeader {
                level: H2,
                target: "comparison-to-other-projects",
                "Comparison with other projects"
            }

            p {
                ""
            }

            AnchorHeader {
                level: H4,
                target: "comparison-to-ui-toolkit",
                "Compared to UI toolkits"
            }

            ul {
                li {
                    dangerous_inner_html: r#"
                        <b><a href="https://flutter.dev/" target="_blank">Flutter</a> / <a href="http://reactnative.dev" target="_blank">React Native</a> / <a href="https://lynxjs.org/" target="_blank">LynxJS</a></b>
                        - 
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b><a href="https://ultralig.ht/">Ultralight</a> / <a href="https://sciter.com/">Sciter</a> / <a href="https://github.com/litehtml/litehtml">litehtml</a></b>
                        - Blitz, Ultralight and Sciter are all lightweight engines with strong support for customisation and integration into a wider rendering engine.
                        Ultralight is a WebKit fork whereas Blitz and Sciter are their own engines (Sciter currently has better support for older CSS2 styles/layout, 
                        Blitz has better support for modern standards like Flexbox and CSS Grid). Ultralight and Sciter have a proprietary licences whereas Blitz 
                        is open source.
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b><a href="https://github.com/nicbarker/clay" target="_blank">Clay</a></b> - Clay is a lightweight C library that inspired Blitz
                    "#
                }
            }

            AnchorHeader {
                level: H4,
                target: "comparison-to-browsers",
                "Compared to browser engines"
            }

            ul {
                li {
                    dangerous_inner_html: r#"
                        <p>
                            <b><a href="https://servo.org" target="_blank">Servo</a> / <a href="https://ladybird.org" target="_blank">Ladybird</a></b>
                            - Blitz, Servo and Ladybird all aim to provide high quality implementations of  web standards (both modern and legacy) that
                            can compete with those in Gecko/Webkit/Blink. 
                        </p>
                        <p>
                            Servo and Ladybird are focussed on creating a “complete” web engine that competes directly with top-tier engines.
                            Blitz would also like to compete on those terms eventually, but has more of a focus on “alternative” use cases which may
                            only need part of a web engine, and benefit from flexible APIs that allow the engine to be customised and extended.
                        </p>
                        <p>
                            Blitz can be used in many scenarios in which Servo and Ladybird are unsuitable. For example, if you want to bring your own renderer
                            or scripting engine.
                        </p>
                        <p>
                            Additionally, Blitz is a less established project at an earlier stage of development than Servo / Ladybird
                        </p>
                    "#
                }
                    li {
                    dangerous_inner_html: r#"
                        <p>
                            <b><a href="https://www.ekioh.com/flow-browser/" target="_blank">Ekioh Flow</a></b>
                            - Flow is another "challenger browser" similar to Servo and Ladybird, and more mature than either. It is also more similar to Blitz in terms of being embeddable.
                            However, it is neither free nor open source, and is only available under a commericial license.
                        </p>
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>Gecko (Firefox) / WebKit (Safari) / Blink (Chrome)</b> - These engines are much more complete than Blitz. Blitz may be able to
                        compete with them eventually, but is unlikely to be able do so anytime soon.
                    "#
                }
            }

            AnchorHeader {
                level: H2,
                target: "license",
                "Licensing"
            }

            p {
                "Blitz is licensed under a variety of permissive open source licenses:"
            }

            ul {
                li {
                    style: "margin-bottom: 6px",
                    "All first-party code in Blitz is dual licensed under Apache 2.0 OR MIT."
                }
                li {
                    style: "margin-bottom: 6px",
                    "All dependencies are available under one the Apache 2.0, MIT, BSD, or Zlib, ISC, CC0, Unicode, BSL and MPL licenses
                    (correct at time of writing). A full list can be generated by running cargo-license on Blitz."
                }
            }

            // AnchorHeader {
            //     level: H2,
            //     target: "dependencies",
            //     "Relationship to dependencies"
            // }

            // p {
            //     "Blitz's approach to modularity allows it to take full advantage of existing libraries where appropriate libraries are available which,
            //     (due to the flourishing Rust crates ecosystem and the legacy of the Servo project), turns out to be quite a lot of places."
            // }



        }
    }
}
