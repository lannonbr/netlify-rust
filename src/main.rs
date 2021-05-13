use bimap::BiMap;
use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::fs::read;
use std::path::Path;
use structopt::StructOpt;
use walkdir::WalkDir;

#[derive(Deserialize, Debug)]
struct Config {
    netlify_auth_token: String,
    netlify_site_id: String,
}

#[derive(StructOpt)]
struct CliFlags {
    #[structopt(parse(from_os_str), long = "path")]
    path: std::path::PathBuf,
    #[structopt(long)]
    prod: bool,
}

#[derive(Serialize, Debug)]
struct CreateDeployArgs {
    files: BiMap<String, String>,
    draft: bool,
}

#[derive(Deserialize, Debug)]
struct CreateDeployResponse {
    id: String,
    required: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = CliFlags::from_args();

    let config = match envy::from_env::<Config>() {
        Ok(config) => config,
        Err(error) => panic!("{:#?}", error),
    };
    let mut hashes = BiMap::new();

    println!("File hashes");
    for entry in WalkDir::new(&args.path) {
        let dir_entry = entry.unwrap();

        if !dir_entry.file_type().is_dir() {
            let path = dir_entry.path();
            let path_suffix = &path.strip_prefix(&args.path.as_path()).unwrap();

            let mut file = read(&path)?;

            let mut hasher = Sha1::new();

            hasher.update(&mut file);

            let hash = hasher.digest().to_string();

            println!("{}: {}", &path_suffix.display(), hash);

            let str = path.to_owned().into_os_string().into_string().unwrap();
            hashes.insert(str, hash);
        }
    }
    println!();

    let create_deploy_args = CreateDeployArgs {
        files: hashes.clone(),
        draft: !args.prod,
    };

    dbg!(&create_deploy_args);

    let client: reqwest::Client = reqwest::Client::new();

    let resp_json = client
        .post(format!(
            "https://api.netlify.com/api/v1/sites/{}/deploys",
            config.netlify_site_id
        ))
        .bearer_auth(&config.netlify_auth_token)
        .json(&create_deploy_args)
        .send()
        .await?
        .json::<CreateDeployResponse>()
        .await?;

    dbg!(&resp_json);

    println!("Files needed to be uploaded: {}", resp_json.required.len());

    for required_hash in resp_json.required {
        let file = hashes.get_by_right(&required_hash).unwrap();

        let required_file_path = &args.path.as_path().join(Path::new(file));

        let file_contents = read(&required_file_path).unwrap();

        client
            .put(format!(
                "https://api.netlify.com/api/v1/deploys/{}/files/{}",
                resp_json.id, file
            ))
            .header("Content-Type", "application/octet-stream")
            .bearer_auth(&config.netlify_auth_token)
            .body(file_contents)
            .send()
            .await?;
    }

    println!("Deploy successful!");

    Ok(())
}
