use dioxus::prelude::*;

use crate::components::Page;

#[component]
pub fn HomePage() -> Element {
    rsx! {
        Page { title: "Home".into(), noframe: true,
            div { style: "
                    height: 100vh;
                    background: url(/static/logo-circle.svg);
                    background-size: 150vw;
                    background-position-y: 60%;
                    background-position-x: center;
                    background-repeat: no-repeat;
                    background-color: #f4e8d2;
                    position: relative;
                    display: grid;
                ",
                div { style: "
                        font-size: 20em;
                        justify-self: center;
                        align-self: start;
                        color: #f4e8d2;
                        font-family: Futura;
                        width: min-content;
                        margin-top: 25vh;
                        grid-row: 1;
                        grid-column: 1;
                    ",
                    "Blitz"
                }
                div { style: "
                        font-size: 3em;
                        justify-self: center;
                        align-self: start;
                        color: #f4e8d2;
                        font-family: Futura;
                        width: max-content;
                        margin-top: 70vh;
                        grid-row: 1;
                        grid-column: 1;
                    ",
                    "A radically modular web engine"
                }
            }
        }
        div { style: "
                height: 200vh;
                background: #f4e8d2;
            " }
    }
}
