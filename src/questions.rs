use crate::admin::Admin;
use crate::apikey::ApiKey;
use crate::backend::{MySqlBackend, Value};
use crate::config::Config;
use crate::email;
use chrono::naive::NaiveDateTime;
use chrono::Local;
use mysql::from_value;
use rocket::form::{Form, FromForm};
use rocket::response::Redirect;
use rocket::State;
use rocket_dyn_templates::Template;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Debug, FromForm)]
pub(crate) struct LectureQuestionSubmission {
    answers: BTreeMap<u64, String>,
}

#[derive(Serialize)]
pub(crate) struct LectureQuestion {
    pub id: u64,
    pub prompt: String,
    pub question_num: u64,
    pub answer: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct LectureQuestionsContext {
    pub lec_id: u8,
    pub title: String,
    pub presenters: String,
    pub questions: Vec<LectureQuestion>,
    pub parent: &'static str,
}

#[derive(Serialize)]
struct LectureAnswer {
    id: u64,
    user: String,
    answer: String,
    time: String,
}

#[derive(Serialize)]
struct LectureAnswersContext {
    lec_id: u8,
    answers: Vec<LectureAnswer>,
    parent: &'static str,
}

#[derive(Serialize)]
struct LectureListEntry {
    id: u64,
    label: String,
    num_qs: u64,
    num_answered: u64,
}

#[derive(Serialize)]
struct LectureListContext {
    admin: bool,
    lectures: Vec<LectureListEntry>,
    parent: &'static str,
}

#[get("/")]
pub(crate) fn leclist(
    apikey: ApiKey,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
    config: &State<Config>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let res = bg.prep_exec("SELECT * FROM lectures_with_question_counts", vec![]);

    let user = apikey.user.clone();
    let admin = config.admins.contains(&user);

    let answers_res = bg.prep_exec(
        "SELECT questions.lecture_id, answers.email, COUNT(answers.id) \
         FROM questions JOIN answers ON (questions.id = answers.question_id) \
         WHERE answers.answer != '' \
         GROUP BY questions.lecture_id, answers.email \
         HAVING answers.email = ?",
        vec![user.into()],
    );

    let mut answers_count: HashMap<u64, u64> = HashMap::new();
    for row in answers_res {
        answers_count.insert(from_value(row[0].clone()), from_value(row[2].clone()));
    }

    let mut lecs: Vec<_> = res
        .into_iter()
        .map(|r| {
            let lec_id = from_value(r[0].clone());
            LectureListEntry {
                id: lec_id,
                label: from_value(r[1].clone()),
                num_qs: if r[2] == Value::NULL {
                    0u64
                } else {
                    from_value(r[2].clone())
                },
                num_answered: *answers_count.get(&lec_id).unwrap_or(&0u64),
            }
        })
        .collect();
    lecs.sort_by(|a, b| a.id.cmp(&b.id));

    drop(bg);

    let ctx = LectureListContext {
        admin: admin,
        lectures: lecs,
        parent: "layout",
    };

    Template::render("leclist", &ctx)
}

#[get("/<num>")]
pub(crate) fn answers(
    _admin: Admin,
    num: u8,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let key: Value = (num as u64).into();
    let res = bg.prep_exec(
        "SELECT answers.email, questions.question_number, answers.answer, answers.submitted_at, questions.lecture_id \
             FROM answers JOIN questions ON (answers.question_id = questions.id) \
             WHERE questions.lecture_id = ? \
             ORDER BY answers.email, questions.question_number",
        vec![key],
    );
    drop(bg);
    let answers: Vec<_> = res
        .into_iter()
        .map(|r| LectureAnswer {
            id: from_value(r[1].clone()),
            user: from_value(r[0].clone()),
            answer: from_value(r[2].clone()),
            time: match r[3] {
                Value::Time(..) | Value::Date(..) => {
                    let date = from_value::<NaiveDateTime>(r[3].clone());
                    date.format("%Y-%m-%d %H:%M:%S%.f").to_string()
                }
                _ => String::from(""),
            },
        })
        .collect();

    let ctx = LectureAnswersContext {
        lec_id: num,
        answers: answers,
        parent: "layout",
    };
    Template::render("answers", &ctx)
}

#[get("/<num>")]
pub(crate) fn questions(
    apikey: ApiKey,
    num: u8,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let key: Value = (num as u64).into();

    let answers_res = bg.prep_exec(
        "SELECT answers.question_id, answers.answer, questions.lecture_id, answers.email \
         FROM answers JOIN questions ON (answers.question_id = questions.id) \
         WHERE questions.lecture_id = ? AND answers.email = ? \
         ORDER BY answers.question_id",
        vec![key.clone(), apikey.user.clone().into()],
    );

    let mut answers = HashMap::new();
    for r in answers_res {
        let qid: u64 = from_value(r[0].clone());
        let atext: String = from_value(r[1].clone());
        answers.insert(qid, atext);
    }

    let res = bg.prep_exec(
        "SELECT id, question, question_number FROM questions WHERE lecture_id = ?",
        vec![key],
    );
    drop(bg);

    let mut qs: Vec<_> = res
        .into_iter()
        .map(|r| {
            let qid: u64 = from_value(r[0].clone());
            let answer = answers.get(&qid).map(|s| s.to_owned());
            LectureQuestion {
                id: qid,
                prompt: from_value(r[1].clone()),
                question_num: from_value(r[2].clone()),
                answer: answer,
            }
        })
        .collect();
    qs.sort_by(|a, b| a.question_num.cmp(&b.question_num));

    let ctx = LectureQuestionsContext {
        lec_id: num,
        title: "".into(),      // not needed here
        presenters: "".into(), // same
        questions: qs,
        parent: "layout",
    };
    Template::render("questions", &ctx)
}

#[post("/<num>", data = "<data>")]
pub(crate) fn questions_submit(
    apikey: ApiKey,
    num: u8,
    data: Form<LectureQuestionSubmission>,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
    config: &State<Config>,
) -> Redirect {
    let mut bg = backend.lock().unwrap();
    let ts: Value = Local::now().naive_local().into();

    // insert/replace all answers.
    for (id, answer) in &data.answers {
        bg.replace(
            "answers(id, email, question_id, answer, submitted_at)",
            vec![
                format!("{}-{}", apikey.user, id).into(),
                apikey.user.clone().into(),
                (*id).into(),
                answer.into(),
                ts.clone(),
            ],
        );
    }

    // Map question id to question number (for emails).
    let id_to_number_res = bg.prep_exec(
        "SELECT id, question_number from questions WHERE lecture_id = ?",
        vec![num.into()],
    );
    let mut id_to_number_map: HashMap<u64, u64> = HashMap::new();
    for row in id_to_number_res {
        id_to_number_map.insert(from_value(row[0].clone()), from_value(row[1].clone()));
    }

    // Construct email.
    let answer_log = format!(
        "{}",
        data.answers
            .iter()
            .map(|(i, t)| format!("Question {}:\n{}", id_to_number_map.get(i).unwrap_or(i), t))
            .collect::<Vec<_>>()
            .join("\n-----\n")
    );

    let mut recipients = if num < 90 {
        config.staff.clone()
    } else {
        config.admins.clone()
    };

    // Get the emails of all presents.
    let presenters_res = bg.prep_exec(
        "SELECT email FROM presenters WHERE lecture_id = ?;",
        vec![num.into()],
    );
    for p in presenters_res {
        recipients.push(from_value(p[0].clone()));
    }

    if config.send_emails {
        email::send(
            bg.log.clone(),
            apikey.user.clone(),
            recipients,
            format!("{} meeting {} questions", config.class, num),
            answer_log,
        )
        .expect("failed to send email");
    }
    drop(bg);

    Redirect::to("/leclist")
}


#[derive(Serialize)]
pub(crate) struct GDPRGet {
    pub user: GDPRUser,
    pub answers: Vec<GDPRAnswer>,
    pub presenters: Vec<GDPRPresenter>,
    pub parent: &'static str,
}

#[derive(Serialize)]
pub(crate) struct GDPRUser {
    pub email: String,
    pub apikey: String,
    pub is_admin: bool,
    pub is_remote: bool,
    pub major: String,
    pub year: u32,
    pub gender: String,
    pub employers_consent: String,
    pub ml_consent: String,
}
impl GDPRUser {
    pub fn new() -> GDPRUser {
      Self {
        email: String::from("anonymous_frank@brown.edu"),
        apikey: String::from("abcdefH##$%12345"),
        is_admin: false,
        is_remote: false,
        major: String::from("CS"),
        year: 1,
        gender: String::from("M"),
        employers_consent: String::from("Yes"),
        ml_consent: String::from("Yes"),
      }
    }
}

#[derive(Serialize)]
pub struct GDPRPresenter {
    pub id: u32,
    pub lecture_id: u32,
    pub email: String,
}
#[derive(Serialize)]
pub struct GDPRAnswer {
    pub id: u32,
    pub email: String,
    pub question_id: u32,
    pub lecture_id: u32,
    pub answer: String,
    pub submitted_at: String,
    pub grade: u32,
}

use rand::Rng;
use mysql::*;
use mysql::prelude::*;


#[get("/gdpr_get")]
pub(crate) fn gdpr_get(
    apikey: ApiKey,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut qmap = HashMap::new();
    let mut answers = Vec::new();
    let mut presenters = Vec::new();

    let mut bg = backend.lock().unwrap();
    let handle = bg.handleme();
    let res = handle.query_iter("SELECT * FROM questions").unwrap();
    for row in res {
      let row = row.unwrap();
      let qid: u32 = from_value(row.get(0).unwrap());
      let lid: u32 = from_value(row.get(1).unwrap());
      qmap.insert(qid, lid);
    }
    
    let mut res = handle.query_iter(format!("GDPR GET users '{}'", apikey.user)).unwrap();
    while let Some(result_set) = res.next_set() {
      let result_set = result_set.unwrap();
      if result_set.columns().as_ref()[0].name_str() == "email" {
        // users
        continue;
      } else if result_set.columns().as_ref().len() == 3 {
        // presenters
        for row in result_set {
          let row = row.unwrap();
          presenters.push(GDPRPresenter {
            id: from_value(row.get(0).unwrap()),
            lecture_id: from_value(row.get(1).unwrap()),
            email: String::from("anonymous_frank@brown.edu"),
          });
        }
      } else {
        // answer
        for row in result_set {
          println!("row {:?}", row);
          let row = row.unwrap();        
          let qid: u32 = from_value(row.get(2).unwrap());
          let mut grade = rand::thread_rng().gen_range(85..100);
          if grade < 92 || grade == 97 {
            grade = 100;
          }
          answers.push(GDPRAnswer {
            id: qid * 25 + (rand::thread_rng().gen_range(0..7) as u32),
            email: String::from("anonymous_frank@brown.edu"),
            question_id: qid,
            lecture_id: *qmap.get(&qid).unwrap(),
            answer: from_value(row.get(3).unwrap()),
            submitted_at: from_value(row.get(4).unwrap()),
            grade: grade,
          });
        }
      }
    }

    // sort answers.
    answers.sort_by(|a, b| {
      if a.lecture_id == b.lecture_id {
        a.question_id.cmp(&b.question_id)
      } else {
        a.lecture_id.cmp(&b.lecture_id)
      }
    });

    let ctx = GDPRGet {
        user: GDPRUser::new(),
        answers,
        presenters,
        parent: "layout",
    };
    Template::render("gdpr", &ctx)
}
