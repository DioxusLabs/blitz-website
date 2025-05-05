use dioxus::prelude::*;

use crate::components::Page;

#[component]
pub fn HomePage() -> Element {
    rsx! {
        Page { title: "A radically modular web engine".into(), noframe: true, transparent_header: false,
            div { style: "
                    // height: calc(100vh - 96px);
                    background-color: white;
                    background-color: #f4e8d2;
                    background: url(/static/logo-circle.svg);
                    background-size: 150%;
                    background-position-y: center;
                    background-position-x: center;
                    background-repeat: no-repeat;
                    position: relative;
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    padding: 40px 0;
                ",

                img {
                    src: "/static/counter-example.png",
                    style: "
                    width: 80%;
                    background-color: transparent;
                    margin: 0 auto;
                    "
                }
            }

            div {
                style: "
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    background-color: #f4e8d2;
                    background-color: white;
                ",

                div {
                    style: "
                        font-size: 3em;
                        color: black;
                        // font-family: Futura;
                        font-family: Jost;
                        font-weight: 500;
                        margin-top: 28px;
                        line-height: 1.5;
                    ",
                    "A "
                    em {
                        "radically modular"
                    }
                    " web engine"
                },

                img {
                    src: "/static/blitz-bubble.svg",
                    style: "
                        width: 70%;
                        background-color: #f4e8d2;
                        background-color: white;
                        margin: 0 auto;
                    "
                }
            }

            div {
                style: "
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    background-color: #f4e8d2;
                    text-align: center;
                ",

                div { style: "
                        font-size: 3em;
                        color: black;
                        // font-family: Futura;
                        font-family: Jost;
                        font-weight: 500;
                        margin-top: 28px;
                        line-height: 1.5;
                    ",
                    "Write once, run everywhere with Dioxus Native"
                },

                p {
                    font_size: "1.4em",
                    max_width: "800px",
                    a {
                        href: "https://github.com/DioxusLabs/dioxus/tree/main/packages/native",
                        "Dioxus Native"
                    }
                    " wraps Blitz and provides a write-once, run everywhere app development experience across all major platforms
                    including web."
                }

                img {
                    src: "/static/counter-example.png",
                    style: "
                    width: 70%;
                    background-color: #f4e8d2;
                    margin: 0 auto;
                ",
                }
            }
        }
        div { style: "
                height: 200vh;
                background: #f4e8d2;
            " }
    }
}
