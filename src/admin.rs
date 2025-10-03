use crate::apikey::ApiKey;
use crate::backend::MySqlBackend;
use crate::config::Config;
use crate::questions::{LectureQuestion, LectureQuestionsContext};
use mysql::from_value;
use rocket::form::Form;
use rocket::http::Status;
use rocket::outcome::IntoOutcome;
use rocket::request::{self, FromRequest, Request};
use rocket::response::Redirect;
use rocket::State;
use rocket_dyn_templates::Template;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct Admin;

#[derive(Debug)]
pub(crate) enum AdminError {
    Unauthorized,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Admin {
    type Error = AdminError;

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        let apikey = request.guard::<ApiKey>().await.unwrap();
        let cfg = request.guard::<&State<Config>>().await.unwrap();

        let res = if cfg.admins.contains(&apikey.user) {
            Some(Admin)
        } else {
            None
        };

        res.into_outcome((Status::Unauthorized, AdminError::Unauthorized))
    }
}

#[derive(Debug, FromForm)]
pub(crate) struct AddLectureQuestionForm {
    q_prompt: String,
}

#[derive(Debug, FromForm)]
pub(crate) struct AdminLecAdd {
    lec_id: u8,
    lec_label: String,
}

#[derive(Debug, FromForm)]
pub(crate) struct AdminLecEdit {
    lec_name: String,
    lec_presenters: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct User {
    email: String,
    apikey: String,
    is_admin: u8,
}

#[derive(Serialize)]
struct UserContext {
    users: Vec<User>,
    parent: &'static str,
}

#[get("/")]
pub(crate) fn lec_add(_adm: Admin) -> Template {
    let mut ctx = HashMap::new();
    ctx.insert("parent", String::from("layout"));
    Template::render("admin/lecadd", &ctx)
}

#[post("/", data = "<data>")]
pub(crate) fn lec_add_submit(
    _adm: Admin,
    data: Form<AdminLecAdd>,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Redirect {
    // insert into MySql if not exists
    let mut bg = backend.lock().unwrap();
    bg.insert(
        "lectures",
        vec![
            (data.lec_id as u64).into(),
            data.lec_label.to_string().into(),
        ],
    );
    drop(bg);

    Redirect::to("/leclist")
}

#[get("/<num>")]
pub(crate) fn lec(_adm: Admin, num: u8, backend: &State<Arc<Mutex<MySqlBackend>>>) -> Template {
    let mut bg = backend.lock().unwrap();
    let lres = bg.prep_exec(
        "SELECT * FROM lectures WHERE lectures.id = ?",
        vec![num.into()],
    );
    let pres = bg.prep_exec(
        "SELECT presenters.email \
         FROM presenters \
         WHERE presenters.lecture_id = ?",
        vec![(num as u64).into()],
    );
    let qres = bg.prep_exec(
        "SELECT questions.id, questions.question, questions.question_number FROM questions WHERE lecture_id = ?",
        vec![(num as u64).into()],
    );
    drop(bg);

    assert_eq!(lres.len(), 1);
    let lec_title: String = from_value(lres[0][1].clone());
    let mut lec_presenters = vec![];
    if pres.len() > 0 {
        for p in pres {
            let presenter: String = from_value(p[0].clone());
            lec_presenters.push(presenter);
        }
    }

    let mut qs: Vec<_> = qres
        .into_iter()
        .map(|r| LectureQuestion {
            id: from_value(r[0].clone()),
            prompt: from_value(r[1].clone()),
            question_num: from_value(r[2].clone()),
            answer: None,
        })
        .collect();
    qs.sort_by(|a, b| a.id.cmp(&b.id));

    let ctx = LectureQuestionsContext {
        lec_id: num,
        title: lec_title,
        presenters: lec_presenters.join(","),
        questions: qs,
        parent: "layout",
    };
    Template::render("admin/lec", &ctx)
}

#[post("/<num>", data = "<data>")]
pub(crate) fn lec_edit_submit(
    _adm: Admin,
    num: u8,
    data: Form<AdminLecEdit>,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Redirect {
    let mut bg = backend.lock().unwrap();
    bg.prep_exec(
        "UPDATE lectures SET label = ? WHERE id = ?",
        vec![data.lec_name.clone().into(), num.into()],
    );
    bg.prep_exec(
        "DELETE FROM presenters WHERE presenters.lecture_id = ?",
        vec![num.into()],
    );
    for presenter in data.lec_presenters.split(",") {
        let presenter = presenter.trim();
        bg.insert(
            "presenters(lecture_id, email)",
            vec![num.into(), presenter.into()],
        );
    }
    drop(bg);

    Redirect::to(format!("/admin/lec/{}", num))
}

#[post("/<num>", data = "<data>")]
pub(crate) fn addq(
    _adm: Admin,
    num: u8,
    data: Form<AddLectureQuestionForm>,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Redirect {
    let mut bg = backend.lock().unwrap();

    // Find question number within lecture.
    let res = bg.prep_exec(
        "SELECT lecture_id, COUNT(id) FROM questions WHERE lecture_id = ? GROUP BY lecture_id",
        vec![(num as u64).into()],
    );
    let question_number = if res.len() > 0 {
        from_value::<u64>(res[0][1].clone()) + 1
    } else {
        1u64
    };

    // Insert question.
    bg.insert(
        "questions(lecture_id, question_number, question)",
        vec![
            (num as u64).into(),
            question_number.into(),
            data.q_prompt.to_string().into(),
        ],
    );
    drop(bg);

    Redirect::to(format!("/admin/lec/{}", num))
}

#[get("/<num>/<qid>")]
pub(crate) fn editq(
    _adm: Admin,
    num: u8,
    qid: u8,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let res = bg.prep_exec(
        "SELECT question, question_number FROM questions WHERE id = ?",
        vec![(qid as u64).into()],
    );
    drop(bg);
    println!("{:?}", res[0][0]);
    println!("{:?}", res[0][1]);

    assert_eq!(res.len(), 1);
    let mut ctx = HashMap::new();
    ctx.insert("id", format!("{}", qid));
    ctx.insert("lec_id", format!("{}", num));
    ctx.insert("lec_qprompt", from_value::<String>(res[0][0].clone()));
    ctx.insert(
        "lec_qnum",
        format!("{}", from_value::<u64>(res[0][1].clone())),
    );
    ctx.insert("parent", String::from("layout"));
    Template::render("admin/lecedit", &ctx)
}

#[post("/editq/<num>/<qid>", data = "<data>")]
pub(crate) fn editq_submit(
    _adm: Admin,
    num: u8,
    qid: u8,
    data: Form<AddLectureQuestionForm>,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Redirect {
    let mut bg = backend.lock().unwrap();
    bg.prep_exec(
        "UPDATE questions SET question = ? WHERE id = ?",
        vec![data.q_prompt.to_string().into(), (qid as u64).into()],
    );
    drop(bg);

    Redirect::to(format!("/admin/lec/{}", num))
}

#[get("/")]
pub(crate) fn get_registered_users(
    _adm: Admin,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let res = bg.prep_exec("SELECT email, is_admin, apikey FROM users", vec![]);
    drop(bg);

    let users: Vec<_> = res
        .into_iter()
        .map(|r| User {
            email: from_value(r[0].clone()),
            apikey: from_value(r[2].clone()),
            is_admin: from_value(r[1].clone()),
        })
        .collect();

    let ctx = UserContext {
        users: users,
        parent: "layout",
    };
    Template::render("admin/users", &ctx)
}

#[derive(Serialize)]
struct GradingContext {
    pub lectures_count: u64,
    pub lectures: Vec<GradingLectureContext>,
    pub users: Vec<GradingUserContext>,
    pub parent: &'static str,
}
#[derive(Serialize)]
struct GradingUserContext {
    pub email: String,
    pub total: f64,
    pub lectures: Vec<u64>,
}
#[derive(Serialize)]
struct GradingLectureContext {
    pub title: String,
    pub id: u64,
    pub questions: u64,
}

#[get("/")]
pub(crate) fn grading(
    _adm: Admin,
    backend: &State<Arc<Mutex<MySqlBackend>>>,
) -> Template {
    let mut bg = backend.lock().unwrap();
    let res = bg.prep_exec("SELECT * FROM lectures", vec![]);

    // Lectures.
    let mut lectures_map = std::collections::BTreeMap::new();
    for row in res {
        let id: u64 = from_value(row[0].clone());
        let title: String = from_value(row[1].clone());
        lectures_map.insert(id, title);
    }
    let mut lectures: Vec<GradingLectureContext> = lectures_map.into_iter().map(
        |(id, title)| GradingLectureContext {
            title,
            id,
            questions: 0
        }
    ).collect();
    let lectures_count = lectures.len() as u64;

    // Questions.
    let res = bg.prep_exec("SELECT id, lecture_id FROM questions", vec![]);
    let mut questions = HashMap::new();
    for row in res {
        let id: u64 = from_value(row[0].clone());
        let lecture_id: u64 = from_value(row[1].clone());
        questions.insert(id, lecture_id);
        for lecture in &mut lectures {
            if lecture.id == lecture_id {
                lecture.questions += 1;
            }
        }
    }

    // Users.
    let res = bg.prep_exec("SELECT email FROM users", vec![]);
    let emails = res.iter().map(|row| from_value(row[0].clone())).collect::<Vec<String>>();

    let mut users_map = HashMap::new();
    for email in &emails {
        users_map.insert(
            email.clone(),
            lectures.iter().map(|l| (l.id, 0)).collect::<HashMap<_, _>>()
        );
    }

    let res = bg.prep_exec("SELECT email, question_id, answer FROM answers", vec![]);
    for row in res {
        let answer: String = from_value(row[2].clone());
        if answer.trim().len() < 10 {
            continue
        }

        let email: String = from_value(row[0].clone());
        let question_id: u64 = from_value(row[1].clone());
        let lecture_id = questions.get(&question_id).unwrap();
        *users_map.get_mut(&email).unwrap().get_mut(lecture_id).unwrap() += 1;
    }

    let mut users = Vec::new();
    for email in emails {
        let lmap = users_map.get(&email).unwrap();

        let mut counts = Vec::new();
        let mut total = 0.0;
        for lecture in &lectures {
            let count = *lmap.get(&lecture.id).unwrap();
            total += count as f64 / lecture.questions as f64;
            counts.push(count);
        }

        users.push(GradingUserContext {
           email,
           total,
           lectures: counts,
        });
    }

    // Can drop now.
    drop(bg);

    let ctx = GradingContext {
        lectures,
        lectures_count,
        users,
        parent: "layout",
    };
    Template::render("admin/grade", &ctx)
}