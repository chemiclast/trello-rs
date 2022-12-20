use reqwest::{Url, UrlError};

pub struct Client {
    pub host: String,
    pub token: String,
    pub key: String,
}

impl Client {
    pub fn new(host: &str, token: &str, key: &str) -> Client {
        Client {
            host: String::from(host),
            token: String::from(token),
            key: String::from(key),
        }
    }

    /// Gets the resultant URL of the Trello Client given some path and additional
    /// parameters. The authentication credentials provided will be included as part
    /// of the generated URL
    /// ```
    /// # use reqwest::UrlError;
    /// # fn main() -> Result<(), UrlError> {
    /// let client = trello::Client {
    ///     host: String::from("https://api.trello.com"),
    ///     token: String::from("some-token"),
    ///     key: String::from("some-key"),
    /// };
    /// let url = client.get_trello_url("/1/me/boards/", &[])?;
    /// assert_eq!(
    ///     url.to_string(),
    ///     "https://api.trello.com/1/me/boards/?key=some-key&token=some-token"
    /// );
    /// let url = client.get_trello_url("/1/boards/some-id/", &[("lists", "open")])?;
    /// assert_eq!(
    ///     url.to_string(),
    ///     "https://api.trello.com/1/boards/some-id/?key=some-key&token=some-token&lists=open",
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_trello_url(&self, path: &str, params: &[(&str, &str)]) -> Result<Url, UrlError> {
        let auth_params: &[(&str, &str)] = &[("key", &self.key), ("token", &self.token)];

        Ok(Url::parse_with_params(
            &format!("{}{}", self.host, path),
            &[auth_params, params].concat(),
        )?)
    }
}
