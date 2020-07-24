// Tokio/Future Imports
use futures::future::ok;
use futures::{Future, Stream};
use tokio_core::reactor::Core;

// Hyper Imports
use hyper::header::{HeaderName, HeaderValue, IF_NONE_MATCH};
use hyper::StatusCode;
use hyper::{self, Body, HeaderMap};
use hyper::{Client, Request};
#[cfg(feature = "rustls")]
type HttpsConnector = hyper_rustls::HttpsConnector<hyper::client::HttpConnector>;
#[cfg(feature = "rust-native-tls")]
use hyper_tls;
#[cfg(feature = "rust-native-tls")]
type HttpsConnector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

// Serde Imports
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json;

// Internal Library Imports
use crate::errors::*;
use crate::gists;
use crate::misc;
use crate::notifications;
use crate::orgs;
use crate::repos;
use crate::users;
use crate::util::url_join;

use std::cell::RefCell;
use std::rc::Rc;

/// Struct used to make calls to the Github API.
pub struct Github {
    token: Option<String>,
    core: Rc<RefCell<Core>>,
    client: Rc<Client<HttpsConnector>>,
}

impl Clone for Github {
    fn clone(&self) -> Self {
        Self {
            token: self.token.clone(),
            core: Rc::clone(&self.core),
            client: Rc::clone(&self.client),
        }
    }
}

new_type!(GetQueryBuilder);

new_type!(PutQueryBuilder);

new_type!(PostQueryBuilder);

new_type!(DeleteQueryBuilder);

new_type!(PatchQueryBuilder);

new_type!(CustomQuery);

exec!(CustomQuery);

pub trait Executor {
    fn execute<T>(self) -> Result<(HeaderMap, StatusCode, Option<T>)>
    where
        T: DeserializeOwned;
}

impl Github {
    /// Create a new Github client struct. It takes a type that can convert into
    /// an &str (`String` or `Vec<u8>` for example). As long as the function is
    /// given a valid API Token your requests will work.
    pub fn new<T>(token: T) -> Result<Self>
    where
        T: ToString,
    {
        let core = Core::new()?;
        #[cfg(feature = "rustls")]
        let client = Client::builder().build(HttpsConnector::new(4));
        #[cfg(feature = "rust-native-tls")]
        let client = Client::builder().build(HttpsConnector::new(4)?);
        Ok(Self {
            token: Some(token.to_string()),
            core: Rc::new(RefCell::new(core)),
            client: Rc::new(client),
        })
    }

    /// Get the currently set Authorization Token
    pub fn get_token(&self) -> &Option<String> {
        &self.token
    }

    /// Change the currently set Authorization Token using a type that can turn
    /// into an &str. Must be a valid API Token for requests to work.
    pub fn set_token<T>(&mut self, token: T)
    where
        T: ToString,
    {
        self.token = Some(token.to_string());
    }

    /// Exposes the inner event loop for those who need
    /// access to it. The recommended way to safely access
    /// the core would be
    ///
    /// ```text
    /// let g = Github::new("API KEY");
    /// let core = g.get_core();
    /// // Handle the error here.
    /// let ref mut core_mut = *core.try_borrow_mut()?;
    /// // Do stuff with the core here. This prevents a runtime failure by
    /// // having two mutable borrows to the core at the same time.
    /// ```
    ///
    /// This is how other parts of the API are implemented to avoid causing your
    /// program to crash unexpectedly. While you could borrow without the
    /// `Result` being handled it's highly recommended you don't unless you know
    /// there is no other mutable reference to it.
    pub fn get_core(&self) -> &Rc<RefCell<Core>> {
        &self.core
    }

    /// Begin building up a GET request to GitHub
    pub fn get(&self) -> GetQueryBuilder {
        self.into()
    }

    /// Begin building up a PUT request with no data to GitHub
    pub fn put_empty(&self) -> PutQueryBuilder {
        self.into()
    }

    /// Begin building up a PUT request with data to GitHub
    pub fn put<T>(&self, body: T) -> PutQueryBuilder
    where
        T: Serialize,
    {
        let mut qb: PutQueryBuilder = self.into();
        if let Ok(mut qbr) = qb.request {
            let serialized = serde_json::to_vec(&body);
            match serialized {
                Ok(json) => {
                    *qbr.get_mut().body_mut() = json.into();
                    qb.request = Ok(qbr);
                }
                Err(_) => {
                    qb.request = Err("Unable to serialize data to JSON".into());
                }
            }
        }
        qb
    }

    /// Begin building up a POST request with data to GitHub
    pub fn post<T>(&self, body: T) -> PostQueryBuilder
    where
        T: Serialize,
    {
        let mut qb: PostQueryBuilder = self.into();
        if let Ok(mut qbr) = qb.request {
            let serialized = serde_json::to_vec(&body);
            match serialized {
                Ok(json) => {
                    *qbr.get_mut().body_mut() = json.into();
                    qb.request = Ok(qbr);
                }
                Err(_) => {
                    qb.request = Err("Unable to serialize data to JSON".into());
                }
            }
        }

        qb
    }

    /// Begin building up a PATCH request with data to GitHub
    pub fn patch<T>(&self, body: T) -> PatchQueryBuilder
    where
        T: Serialize,
    {
        let mut qb: PatchQueryBuilder = self.into();
        if let Ok(mut qbr) = qb.request {
            let serialized = serde_json::to_vec(&body);
            match serialized {
                Ok(json) => {
                    *qbr.get_mut().body_mut() = json.into();
                    qb.request = Ok(qbr);
                }
                Err(_) => {
                    qb.request = Err("Unable to serialize data to JSON".into());
                }
            }
        }
        qb
    }

    /// Begin building up a DELETE request with data to GitHub
    pub fn delete<T>(&self, body: T) -> DeleteQueryBuilder
    where
        T: Serialize,
    {
        let mut qb: DeleteQueryBuilder = self.into();

        if let Ok(mut qbr) = qb.request {
            let serialized = serde_json::to_vec(&body);
            match serialized {
                Ok(json) => {
                    *qbr.get_mut().body_mut() = json.into();
                    qb.request = Ok(qbr);
                }
                Err(_) => {
                    qb.request = Err("Unable to serialize data to JSON".into());
                }
            }
        }
        qb
    }

    /// Begin building up a DELETE request without data to GitHub
    pub fn delete_empty(&self) -> DeleteQueryBuilder {
        self.into()
    }
}

impl<'g> GetQueryBuilder<'g> {
    /// Pass in an endpoint not covered by the API in the form of the following:
    ///
    /// ```no_test
    /// # Don't have the beginning / in it
    /// repos/mgattozzi/github-rs
    /// ```
    ///
    /// It can be whatever endpoint or url string that's needed. This will allow
    /// you to get functionality out of the library as items are still added or
    /// if you need access to a hidden endpoint.
    func_client!(custom_endpoint, CustomQuery, endpoint_str);

    /// Query the emojis endpoint
    func_client!(emojis, misc::get::Emojis<'g>);

    /// Query the events endpoint
    func_client!(events, misc::get::Events<'g>);

    /// Query the feeds endpoint
    func_client!(feeds, misc::get::Feeds<'g>);

    /// Query the gitignore endpoint
    func_client!(gitignore, misc::get::Gitignore<'g>);

    /// Query the meta endpoint
    func_client!(meta, misc::get::Meta<'g>);

    /// Query the rate limit endpoint
    func_client!(rate_limit, misc::get::RateLimit<'g>);

    /// Query the user endpoint
    func_client!(user, users::get::User<'g>);

    /// Query the users endpoint
    func_client!(users, users::get::Users<'g>);

    /// Query the repos endpoint
    func_client!(repos, repos::get::Repos<'g>);

    /// Query the gists endpoint
    func_client!(gists, gists::get::Gists<'g>);

    /// Query the orgs endpoint
    func_client!(orgs, orgs::get::Orgs<'g>);

    /// Query the organizations endpoint
    func_client!(organizations, misc::get::Organizations<'g>);

    /// Query the notifications endpoint
    func_client!(notifications, notifications::get::Notifications<'g>);

    /// Add an etag to the headers of the request
    pub fn set_etag(mut self, tag: impl Into<HeaderValue>) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(IF_NONE_MATCH, tag.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}

impl<'g> PutQueryBuilder<'g> {
    /// Pass in an endpoint not covered by the API in the form of the following:
    ///
    /// ```no_test
    /// # Don't have the beginning / in it
    /// repos/mgattozzi/github-rs
    /// ```
    ///
    /// It can be whatever endpoint or url string that's needed. This will allow
    /// you to get functionality out of the library as items are still added or
    /// if you need access to a hidden endpoint.
    func_client!(custom_endpoint, CustomQuery, endpoint_str);
    func_client!(user, users::put::User<'g>);
    func_client!(gists, gists::put::Gists<'g>);
    func_client!(notifications, notifications::put::Notifications<'g>);

    /// Add an etag to the headers of the request
    pub fn set_etag(mut self, tag: impl Into<HeaderValue>) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(IF_NONE_MATCH, tag.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}

impl<'g> DeleteQueryBuilder<'g> {
    /// Pass in an endpoint not covered by the API in the form of the following:
    ///
    /// ```no_test
    /// # Don't have the beginning / in it
    /// repos/mgattozzi/github-rs
    /// ```
    ///
    /// It can be whatever endpoint or url string that's needed. This will allow
    /// you to get functionality out of the library as items are still added or
    /// if you need access to a hidden endpoint.
    func_client!(custom_endpoint, CustomQuery, endpoint_str);
    func_client!(user, users::delete::User<'g>);
    func_client!(gists, gists::delete::Gists<'g>);
    func_client!(notifications, notifications::delete::Notifications<'g>);

    /// Add an etag to the headers of the request
    pub fn set_etag(mut self, tag: impl Into<HeaderValue>) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(IF_NONE_MATCH, tag.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}

impl<'g> PostQueryBuilder<'g> {
    /// Pass in an endpoint not covered by the API in the form of the following:
    ///
    /// ```no_test
    /// # Don't have the beginning / in it
    /// repos/mgattozzi/github-rs
    /// ```
    ///
    /// It can be whatever endpoint or url string that's needed. This will allow
    /// you to get functionality out of the library as items are still added or
    /// if you need access to a hidden endpoint.
    func_client!(custom_endpoint, CustomQuery, endpoint_str);
    func_client!(user, users::post::User<'g>);
    func_client!(repos, repos::post::Repos<'g>);
    func_client!(gists, gists::post::Gists<'g>);

    /// Add an etag to the headers of the request
    pub fn set_etag(mut self, tag: impl Into<HeaderValue>) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(IF_NONE_MATCH, tag.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}

impl<'g> PatchQueryBuilder<'g> {
    /// Pass in an endpoint not covered by the API in the form of the following:
    ///
    /// ```no_test
    /// # Don't have the beginning / in it
    /// repos/mgattozzi/github-rs
    /// ```
    ///
    /// It can be whatever endpoint or url string that's needed. This will allow
    /// you to get functionality out of the library as items are still added or
    /// if you need access to a hidden endpoint.
    func_client!(custom_endpoint, CustomQuery, endpoint_str);
    func_client!(user, users::patch::User<'g>);
    func_client!(gists, gists::patch::Gists<'g>);
    func_client!(notifications, notifications::patch::Notifications<'g>);

    /// Add an etag to the headers of the request
    pub fn set_etag(mut self, tag: impl Into<HeaderValue>) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(IF_NONE_MATCH, tag.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}

// From derivations of Github to the given type using a certain
// request method
from!(
    @GetQueryBuilder
        => "GET"
    @PutQueryBuilder
        => "PUT"
    @PostQueryBuilder
        => "POST"
    @PatchQueryBuilder
        => "PATCH"
    @DeleteQueryBuilder
        => "DELETE"
);

// Custom Url based from impls
from!(
    @GetQueryBuilder
       => CustomQuery
    @PutQueryBuilder
       => CustomQuery
    @PostQueryBuilder
       => CustomQuery
    @PatchQueryBuilder
       => CustomQuery
    @DeleteQueryBuilder
       => CustomQuery
);

impl<'a> CustomQuery<'a> {
    /// Set custom header for request.
    /// Useful for custom headers (sometimes using in api preview).
    pub fn set_header(
        mut self,
        header_name: impl Into<HeaderName>,
        accept_header: impl Into<HeaderValue>,
    ) -> Self {
        match self.request {
            Ok(mut req) => {
                req.get_mut()
                    .headers_mut()
                    .insert(header_name.into(), accept_header.into());
                self.request = Ok(req);
                self
            }
            Err(_) => self,
        }
    }
}
