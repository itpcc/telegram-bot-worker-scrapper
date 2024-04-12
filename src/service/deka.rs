use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    error,
    model::{
        DekaInfo, DekaMetadata, MessagePayload, TGDeka, TGDekaNumber, TGDekaSearch, TGResponse,
        TGResponseErr, TGResponseOkay,
    },
    util,
};
use fantoccini::{elements::Form, Client, ClientBuilder, Locator};
use futures::future::join_all;
use itertools::Itertools;
use regex::Regex;
use reqwest;
use scraper::{Html, Selector};
use snafu::{OptionExt, ResultExt};
use tokio::{
    io::AsyncWriteExt,
    sync::{broadcast, mpsc, Mutex},
};

use url::Url;

async fn dekasuksa_deka_exec(q: String) -> util::Result<Option<Vec<DekaInfo>>> {
    let mut url = Url::parse("https://www.dekasuksa.com/search").context(error::URLSnafu)?;
    url.query_pairs_mut().append_pair("q", &q);

    // Query from the web
    let html = reqwest::get(url.to_string())
        .await
        .context(error::ReqwestSnafu)?
        .text()
        .await
        .context(error::ReqwestSnafu)?;
    // Selectors
    let (slct_blog_post, slct_title_a, slct_title, slct_cntn) = match (
        Selector::parse(r#".blog-posts .blog-post"#),
        Selector::parse(r#".post-title a"#),
        Selector::parse(r#"h1.post-title"#),
        Selector::parse(r#".post-body.post-content"#),
    ) {
        (Ok(s1), Ok(s2), Ok(s3), Ok(s4)) => (s1, s2, s3, s4),
        _ => return Ok(None),
    };

    let document = Html::parse_document(html.as_str());

    let deka_links = document
        .select(&slct_blog_post)
        .map(|deka| {
            deka.select(&&slct_title_a)
                .next()?
                .value()
                .attr("href")
                .and_then(|l| Some(l.to_string()))
        })
        .flatten()
        .collect::<Vec<String>>();

    if deka_links.is_empty() {
        return Ok(None);
    }

    let deka_resp = join_all(
        deka_links
            .iter()
            .map(|link| reqwest::get(link))
            .collect_vec(),
    )
    .await
    .into_iter()
    .flatten()
    .filter(|res| res.status().as_u16() == 200)
    .map(|res| res.text())
    .collect_vec();

    let mut deka_res = join_all(deka_resp)
        .await
        .into_iter()
        .map(|txt| {
            txt.context(error::ReqwestSnafu).and_then(|t| {
                let doc = Html::parse_document(t.as_str());
                let post_header = doc
                    .select(&slct_title)
                    .next()
                    .context(error::EmptySnafu)?
                    .text()
                    .collect::<Vec<_>>()
                    .concat();
                let post_content = doc
                    .select(&slct_cntn)
                    .next()
                    .context(error::EmptySnafu)?
                    .text()
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut meta_law: Option<String> = None;
                let mut short_note = String::new();
                let mut long_note: Option<String> = None;

                for txt_line in post_content.lines() {
                    let txt_stp = txt_line.trim();

                    if meta_law.is_none() {
                        if long_note.is_none() {
                            if txt_stp == "เพิ่มเติม" {
                                long_note = Some(String::new());
                            } else {
                                short_note.push_str(&(txt_stp.to_string() + "\n"));
                            }
                        } else if txt_stp.is_empty() && meta_law.is_none() {
                            meta_law = Some(String::new());
                        } else if txt_stp.len() < 100 && !short_note.is_empty() {
                            meta_law
                                .as_mut()
                                .and_then(|ml| Some(ml.to_owned() + txt_stp + "\n"));
                        } else if long_note.is_some() {
                            long_note
                                .as_mut()
                                .and_then(|ln| Some(ln.to_owned() + txt_stp + "\n"));
                        }
                    } else {
                        meta_law
                            .as_mut()
                            .and_then(|ml| Some(ml.to_owned() + txt_stp + "\n"));
                    }
                }

                Ok(DekaInfo {
                    deka_no: post_header
                        .replace("๐", "0")
                        .replace("๑", "1")
                        .replace("๒", "2")
                        .replace("๓", "3")
                        .replace("๔", "4")
                        .replace("๕", "5")
                        .replace("๖", "6")
                        .replace("๗", "7")
                        .replace("๘", "8")
                        .replace("๙", "9"),
                    short_note: short_note,
                    long_note,
                    metadata: DekaMetadata {
                        law: meta_law.unwrap_or_default(),
                        source: "เว็บไซต์ฎีกาศึกษา".to_string(),
                    },
                })
            })
        })
        .collect_vec();

    for (i, dl) in deka_links.iter().enumerate() {
        deka_res.get_mut(i).and_then(|dr| {
            Some(dr.as_mut().and_then(|dri| {
                dri.metadata.source.push_str(format!(" {}", dl).as_str());
                Ok(dri)
            }))
        });
    }

    let deka_res_final = deka_res.into_iter().flatten().collect_vec();

    if deka_res_final.is_empty() {
        return Ok(None);
    }

    Ok(Some(deka_res_final))
}

async fn spc_select_option(
    client: &Client,
    select_selector: &str,
    label: &str,
) -> util::Result<()> {
    client
        .find(Locator::Css(select_selector))
        .await
        .context(error::FantocciniCmdSnafu)?
        .select_by_label(label)
        .await
        .context(error::FantocciniCmdSnafu)?;

    Ok(())
}

async fn spc_click(client: &Client, selector: &str) -> util::Result<()> {
    client
        .find(Locator::Css(selector))
        .await
        .context(error::FantocciniCmdSnafu)?
        .click()
        .await
        .context(error::FantocciniCmdSnafu)?;

    Ok(())
}

async fn spc_input(form: &Form, selector: &str, value: &str) -> util::Result<()> {
    form.set(Locator::Css(selector), value)
        .await
        .context(error::FantocciniCmdSnafu)?;

    Ok(())
}

async fn spc_text(client: &Client, selector: &str) -> util::Result<String> {
    client
        .find(Locator::Css(selector))
        .await
        .context(error::FantocciniCmdSnafu)?
        .text()
        .await
        .context(error::FantocciniCmdSnafu)
}

// Supreme courts
async fn spc_deka_init(client: &Client) -> util::Result<()> {
    client
        .goto("http://deka.supremecourt.or.th/")
        .await
        .context(error::FantocciniCmdSnafu)?;

    Ok(())
}

async fn spc_deka_exec(
    client: &Client,
    with_long_note: bool,
) -> util::Result<Option<Vec<DekaInfo>>> {
    tracing::info!("spc_deka_exec | Wait Result");
    client
        .wait()
        .at_most(core::time::Duration::from_secs(30))
        .for_url(
            client
                .current_url()
                .await
                .and_then(|mut url| {
                    url.set_path("/search");
                    Ok(url)
                })
                .context(error::FantocciniCmdSnafu)?,
        )
        .await
        .context(error::FantocciniCmdSnafu)?;

    tracing::debug!("spc_deka_exec | Wait for content load");
    client
        .wait()
        .for_element(Locator::Css("#deka_result_info"))
        .await
        .context(error::FantocciniCmdSnafu)?;

    tracing::info!("spc_deka_exec | Compiling Result");

    if with_long_note {
        // Tick show long note
        spc_click(&client, "#btn-show-result-item").await?;
        client
            .wait()
            .for_element(Locator::Id("show_item_long_text"))
            .await
            .context(error::FantocciniCmdSnafu)?;
        spc_click(&client, r#"label[for="show_item_long_text"]"#).await?;
    }

    let deka_res_ftr = client
        .find_all(Locator::Css("#deka_result_info li.result"))
        .await
        .context(error::FantocciniCmdSnafu)?
        .into_iter()
        .map(|deka| async move {
            let dkn_regex = Regex::new(r"^(.*)\s+(<dkn>.*)$").context(error::RegexSnafu)?;
            let dkn_txt = deka
                .find(Locator::Css(".item_deka_no input[type=hidden]"))
                .await
                .context(error::FantocciniCmdSnafu)?
                .attr("value")
                .await
                .context(error::FantocciniCmdSnafu)?
                .unwrap_or_default();

            let deka_info = DekaInfo {
                deka_no: dkn_regex
                    .captures(&dkn_txt)
                    .and_then(|dkn_cpt| dkn_cpt.name("dkn"))
                    .and_then(|dkn_mtch| {
                        let txt = dkn_mtch.as_str().trim().to_string();

                        if txt == "" {
                            return None;
                        }

                        Some(txt)
                    })
                    .unwrap_or_else(|| dkn_txt),
                short_note: spc_text(&client, ".item_short_text").await?,
                long_note: spc_text(&client, ".item_long_text")
                    .await
                    .ok()
                    .and_then(|dkn_mtch| {
                        let txt = dkn_mtch.as_str().trim().to_string();

                        if txt.is_empty() {
                            return None;
                        }

                        Some(txt)
                    }),
                metadata: DekaMetadata {
                    law: spc_text(&client, ".item_law>ul").await?,
                    source: spc_text(&client, ".item_source>ul").await?,
                },
            };
            println!("deka_info -> {:?}", deka_info);

            Ok::<DekaInfo, error::Error>(deka_info)
        });

    let mut deka_res = join_all(deka_res_ftr)
        .await
        .into_iter()
        .map(|deka| match deka {
            Ok(dk) => Some(dk),
            _ => None,
        })
        .flatten()
        .collect_vec();

    if deka_res.is_empty() {
        return Ok(None);
    }

    if ! with_long_note ||
		// If SPC provide us all long text, there's no need to print
		deka_res.iter().find(|dr| dr.long_note.is_none()).is_none()
    {
        return Ok(Some(deka_res));
    }

    let mut print_url = client
        .current_url()
        .await
        .context(error::FantocciniCmdSnafu)?;

    print_url.set_path("/printing/dekaall");
    spc_click(&client, "#choose_all_deka").await?;
    spc_click(&client, "#print_choose_deka").await?;
    tracing::info!("deka::spc_deka_exec | Wait Print page");

    client
        .wait()
        .at_most(Duration::from_secs(30))
        .for_url(print_url)
        .await
        .context(error::FantocciniCmdSnafu)?;

    // TODO check page

    let deka_res_long = client
        .find_all(Locator::Css("#print-layer page"))
        .await
        .context(error::FantocciniCmdSnafu)?;

    let dkn_regex = Regex::new(r"^คำ.*ศาลฎีกาที่\s+(<dkn>.*)$").context(error::RegexSnafu)?;

    for elm in deka_res_long.iter() {
        let mut long_deka = String::new();
        let mut deka_no: Option<String> = None;

        if let Ok(long_elm) = elm.find_all(Locator::Css(".row>.col-lg-12")).await {
            for le in long_elm {
                if let Ok(lt_txt) = le.text().await {
                    if lt_txt.contains("ศาลฎีกาวินิจฉัยว่า") {
                        long_deka.push_str(&lt_txt);
                    }
                }

                if let Ok(dkn_elms) = elm.find_all(Locator::Css("div>p")).await {
                    for dkn_elm in dkn_elms {
                        if let Ok(dkn_txt) = dkn_elm.text().await {
                            if let Some(dkn_cpt) = dkn_regex.captures(&dkn_txt) {
                                if let Some(dkn_mtch) = dkn_cpt.name("dkn") {
                                    deka_no.replace(dkn_mtch.as_str().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(dkn) = deka_no {
            deka_res
                .iter_mut()
                .find(|dr| dr.deka_no == dkn)
                .and_then(|dr| dr.long_note.replace(long_deka));
        }
    }

    Ok(Some(deka_res))
}

async fn spc_screenshot(client: &Client, filepath: &str) -> util::Result<()> {
    let raw_data = client
        .screenshot()
        .await
        .context(error::FantocciniCmdSnafu)?;
    let filepath_full = filepath.replacen(
        ".png",
        &format!(
            "-{}.png",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context(error::SystemTimeSnafu)?
                .as_secs()
        ),
        1,
    );
    let mut fs = tokio::fs::File::create(filepath_full)
        .await
        .context(error::IOSnafu)?;
    fs.write_all(&raw_data).await.context(error::IOSnafu)?;
    fs.flush().await.context(error::IOSnafu)?;

    Ok(())
}

async fn spc_deka_no(
    client: &Client,
    deka_params: TGDekaNumber,
    with_screenshot: bool,
) -> util::Result<Option<Vec<DekaInfo>>> {
    spc_deka_init(client).await?;
    tracing::debug!("deka::spc_deka_no | Filling case no");

    let spc_form = client
        .form(Locator::Id("basic_search"))
        .await
        .context(error::FantocciniCmdSnafu)?;

    spc_select_option(&client, "#search_doctype", "คำพิพากษาศาลฎีกา").await?;
    spc_input(&spc_form, "#search_deka_no", &deka_params.deka_serial).await?;
    spc_input(
        &spc_form,
        "#search_deka_start_year",
        &deka_params.deka_year.to_string(),
    )
    .await?;
    spc_input(
        &spc_form,
        "#search_deka_end_year",
        &deka_params.deka_year.to_string(),
    )
    .await?;

    if with_screenshot {
        spc_screenshot(
            &client,
            &format!(
                "./memo/tests/deka.supremecourt-no-form-{}-{}.png",
                deka_params.deka_serial, deka_params.deka_year
            ),
        )
        .await?;
    }

    spc_click(&client, "#submit_search_deka").await?;

    let res = spc_deka_exec(&client, deka_params.with_long_note).await;

    if with_screenshot {
        spc_screenshot(
            &client,
            format!(
                "./memo/tests/deka.supremecourt-no-{}-{}.png",
                deka_params.deka_serial, deka_params.deka_year
            )
            .as_str(),
        )
        .await?;
    }

    client
        .close_window()
        .await
        .context(error::FantocciniCmdSnafu)?;

    res
}

async fn spc_deka_search(
    client: &Client,
    deka_params: TGDekaSearch,
    with_screenshot: bool,
) -> util::Result<Option<Vec<DekaInfo>>> {
    spc_deka_init(client).await?;
    let keyword_cmpl = deka_params.search_words.join(" .และ. ");
    tracing::debug!("deka::spc_deka_search | Filling Info");

    client
        .set_window_size(1920, 1080)
        .await
        .context(error::FantocciniCmdSnafu)?;
    if let Some(law_name) = deka_params.search_law {
        spc_click(&client, "#search-tab a[href=\"#advance-search\"]").await?;
        client
            .wait()
            .at_most(Duration::from_secs(10))
            .for_element(Locator::Id("advance-search"))
            .await
            .context(error::FantocciniCmdSnafu)?;
        let spc_form = client
            .form(Locator::Id("adv_search"))
            .await
            .context(error::FantocciniCmdSnafu)?;
        spc_select_option(&client, "#adv_search_doctype", "คำพิพากษาศาลฎีกา").await?;
        spc_input(&spc_form, "#adv_search_word_stext_and_ltext", &keyword_cmpl).await?;
        spc_click(&client, "#adv_search_temp_law_name").await?;
        spc_input(&spc_form, "#adv_search_temp_law_name", &law_name).await?;
        // Wait for autocomplete dialog
        client
            .wait()
            .at_most(Duration::from_secs(10))
            .for_element(Locator::Css("ul.ui-autocomplete"))
            .await
            .context(error::FantocciniCmdSnafu)?;

        // Select law from autocomplete
        for el in client
            .find_all(Locator::Css("ul.ui-autocomplete li a"))
            .await
            .context(error::FantocciniCmdSnafu)?
        {
            if let Ok(el_txt) = el.text().await {
                if el_txt.contains(&law_name) {
                    el.click().await.context(error::FantocciniCmdSnafu)?;
                }
            }
        }

        if let Some(law_no) = deka_params.search_law_no {
            spc_input(&spc_form, "#adv_search_temp_law_section", &law_no).await?;
        }

        if let Some(case_from) = deka_params.case_from {
            spc_input(
                &spc_form,
                "#adv_search_deka_start_year",
                &case_from.to_string(),
            )
            .await?;
            spc_input(
                &spc_form,
                "#adv_search_deka_end_year",
                &deka_params.case_to.unwrap_or_else(|| case_from).to_string(),
            )
            .await?;
        }

        client
            .execute(r#"window.scrollTo(0, document.body.scrollHeight);"#, vec![])
            .await
            .context(error::FantocciniCmdSnafu)?;
        spc_click(&client, "#submit_adv_search_deka").await?;
    } else {
        let spc_form = client
            .form(Locator::Id("basic_search"))
            .await
            .context(error::FantocciniCmdSnafu)?;
        spc_select_option(&client, "#search_doctype", "คำพิพากษาศาลฎีกา").await?;
        spc_input(&spc_form, "#search_word", &keyword_cmpl).await?;

        if let Some(case_from) = deka_params.case_from {
            spc_input(&spc_form, "#search_deka_start_year", &case_from.to_string()).await?;
            spc_input(
                &spc_form,
                "#search_deka_end_year",
                &deka_params.case_to.unwrap_or_else(|| case_from).to_string(),
            )
            .await?;
        }

        spc_click(&client, "#submit_search_deka").await?;
    }

    let res = spc_deka_exec(&client, deka_params.with_long_note).await;

    if with_screenshot {
        spc_screenshot(
            &client,
            &format!("./memo/tests/deka.supremecourt-search-{}.png", keyword_cmpl),
        )
        .await?;
    }
    client
        .close_window()
        .await
        .context(error::FantocciniCmdSnafu)?;

    res
}

async fn on_message(client: &Arc<Mutex<Client>>, pld: MessagePayload) -> TGResponse {
    tracing::debug!("deka::deka_thread | Receive message {:?}", pld);

    // Find DekaSuksa first
    let res = match pld.info {
        TGDeka::Number(deka) => {
            match dekasuksa_deka_exec(format!("{}/{}", deka.deka_serial, deka.deka_year)).await {
                Ok(dk) => Ok(dk),
                Err(e) => {
					tracing::debug!("deka::dekasuksa | Fetch error {:?}", e);
                    let clnt = client.lock().await;
                    spc_deka_no(&clnt, deka, false).await
                }
            }
        }
        TGDeka::Search(deka) => {
            match dekasuksa_deka_exec(format!(
                "{} {} {} {}",
                deka.search_words.join(" "),
                deka.search_law.clone().unwrap_or_default(),
                deka.search_law_no.clone().unwrap_or_default(),
                match deka.case_from {
                    Some(cf) => cf.to_string(),
                    _ => "".to_string(),
                }
            ))
            .await
            {
                Ok(dk) => Ok(dk),
                _ => {
                    let clnt = client.lock().await;
                    spc_deka_search(&clnt, deka.clone(), false).await
                }
            }
        }
    };

    match res {
        Ok(res) => TGResponse::Okay(TGResponseOkay {
            from: "deka".to_string(),
            message: pld.message,
            result: res,
        }),
        Err(e) => TGResponse::Err(TGResponseErr {
            from: "deka".to_string(),
            message: pld.message,
            error: format!("Unable to find Deka:\n{:?}", e),
        }),
    }
}

pub async fn deka_thread(
    mut sig_rx: broadcast::Receiver<()>,
    mut ws_rx: mpsc::Receiver<MessagePayload>,
    tg_tx: mpsc::Sender<TGResponse>,
) -> () {
    let c = match ClientBuilder::native()
        .connect("http://localhost:4444")
        .await
        .context(error::FantocciniSessionSnafu)
    {
        Ok(c) => match c.persist().await {
            Ok(_) => c,
            Err(e) => {
                tracing::warn!("deka::deka_thread | Persist error: {:?}", e);
                return ();
            }
        },
        Err(e) => {
            tracing::warn!("deka::deka_thread | Browser launch error: {:?}", e);
            return ();
        }
    };

    let client = Arc::new(Mutex::new(c));

    loop {
        tokio::select! {
            Ok(_) = sig_rx.recv() => {
                tracing::info!("deka::deka_thread | Shutdown signal received.");
                break;
            },
            Some(pld) = ws_rx.recv() => {
                let tg_msg = on_message(&client, pld).await;
                if let Err(e) = tg_tx.send(tg_msg.clone()).await {
                    tracing::warn!(
                        "deka::deka_thread | Unable to tx Telegram: {:?}\nMessage: {:?}",
                        e,
                        tg_msg
                    );
                }
            },
        }
    }

    let _ = Arc::try_unwrap(client).unwrap().into_inner().close().await;
}

#[cfg(test)]
mod tests {
    use crate::model::{TGChat, TGMessageInfo, TGMessgae, TGUser};
    use fantoccini::wd::Capabilities;

    use super::*;

    async fn get_browser() -> Client {
        let cap: Capabilities =
            serde_json::from_str(r#"{"moz:firefoxOptions":{"args":["--headless"]}}"#).unwrap();
        ClientBuilder::native()
            .capabilities(cap)
            .connect("http://localhost:4444")
            .await
            .context(error::FantocciniSessionSnafu)
            .unwrap()
    }

    #[tokio::test]
    async fn spc_init_test() {
        let client = get_browser().await;
        spc_deka_init(&client).await.unwrap();
        spc_screenshot(&client, "./memo/tests/deka.supremecourt-test.png")
            .await
            .unwrap();

        assert_eq!(
            client.current_url().await.unwrap().to_string(),
            "http://deka.supremecourt.or.th/"
        );
        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn spc_deka_no_test() {
        let client = get_browser().await;
        let deka_res = spc_deka_no(
            &client,
            TGDekaNumber {
                deka_serial: "264".to_string(),
                deka_year: 2567,
                with_long_note: false,
            },
            true,
        )
        .await;

        assert!(deka_res.is_ok());

        if let Ok(dr) = deka_res {
            assert!(dr.is_some());

            if let Some(drs) = dr {
                assert!(drs.first().unwrap().deka_no.ends_with("264/2567"));
            }
        }

        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn spc_deka_no_long_test() {
        let client = get_browser().await;
        let deka_res = spc_deka_no(
            &client,
            TGDekaNumber {
                deka_serial: "264".to_string(),
                deka_year: 2567,
                with_long_note: true,
            },
            true,
        )
        .await;
        println!("deka_res: {:?}", deka_res);
        assert!(deka_res.is_ok());

        if let Ok(dr) = deka_res {
            assert!(dr.is_some());

            if let Some(drs) = dr {
                assert!(drs.first().unwrap().deka_no.ends_with("264/2567"));
                assert!(drs.first().unwrap().long_note.is_some());
            }
        }

        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn spc_deka_search_test() {
        let client = get_browser().await;
        let deka_res = spc_deka_search(
            &client,
            TGDekaSearch {
                search_law: Some("ประมวลกฎหมายแพ่งและพาณิชย์".into()),
                search_words: ["เช่าซื้อ".to_string(), "รถยนต์".to_string()].to_vec(),
                search_law_no: Some("420".to_string()),
                case_from: Some(2560),
                case_to: Some(2567),
                with_long_note: false,
            },
            true,
        )
        .await;
        println!("deka_res: {:?}", deka_res);
        assert!(deka_res.is_ok());

        if let Ok(dr) = deka_res {
            assert!(dr.is_some());

            if let Some(drs) = dr {
                assert!(drs.first().is_some());
            }
        }

        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn dks_test() {
        let deka_res = dekasuksa_deka_exec("3853/2566".to_string()).await;

        println!("deka_res: {:?}", deka_res);
        assert!(deka_res.is_ok());

        if let Ok(dr) = deka_res {
            assert!(dr.is_some());

            if let Some(drs) = dr {
                assert!(drs.first().unwrap().deka_no.contains("3853/2566"));
            }
        }
    }

    #[tokio::test]
    async fn deka_thread_test() {
        let client = Arc::new(Mutex::new(get_browser().await));
        let resp = on_message(
            &client,
            MessagePayload {
                message: TGMessgae {
                    update_id: 1234,
                    message: TGMessageInfo {
                        message_id: 1234,
                        from: TGUser {
                            id: 123456,
                            is_bot: false,
                            first_name: "test".to_string(),
                            last_name: None,
                            username: "tester".to_string(),
                            language_code: "EN".to_string(),
                        },
                        chat: TGChat {
                            id: 1234568,
                            first_name: "test".to_string(),
                            username: "tester".to_string(),
                            chat_type: "general".to_string(),
                        },
                        date: 12334554,
                        text: "ฎีกา 264/2567".to_string(),
                    },
                },
                info: TGDeka::Number(TGDekaNumber {
                    deka_serial: "264".to_string(),
                    deka_year: 2567,
                    with_long_note: false,
                }),
            },
        )
        .await;
        assert!(matches!(resp, TGResponse::Okay(_)));
    }
}
