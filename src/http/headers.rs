//! Headers API implementation
//!
//! WinterCG-compliant Headers type that wraps HTTP headers with
//! case-insensitive name handling per RFC 7230.
//!
//! # Decisions
//!
//! - **D-07:** Case-insensitive header names, normalized to lowercase per RFC 7230
//! - **D-08:** Multiple values combine with commas, except Set-Cookie which stays separate

use std::collections::HashMap;

/// WinterCG-compliant Headers implementation
///
/// Stores headers in a HashMap with lowercase keys for case-insensitive
/// matching per RFC 7230. Multiple values are stored as vectors.
///
/// Special handling for Set-Cookie per D-08: cookie values remain separate
/// rather than being comma-combined, preserving browser compatibility.
#[derive(Debug, Clone, Default)]
pub struct NanoHeaders {
    headers: HashMap<String, Vec<String>>, // lowercase name -> values
}

impl NanoHeaders {
    /// Create a new empty Headers object
    ///
    /// # Example
    ///
    /// ```
    /// use nano::http::NanoHeaders;
    ///
    /// let headers = NanoHeaders::new();
    /// assert!(!headers.has("content-type"));
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from axum header map
    ///
    /// Converts axum's HeaderMap into NanoHeaders, normalizing names
    /// to lowercase and storing all values.
    ///
    /// # Arguments
    ///
    /// * `axum_headers` - The axum HeaderMap to convert
    ///
    /// # Returns
    ///
    /// A new `NanoHeaders` with all headers from the axum map
    pub fn from_axum_headers(axum_headers: &axum::http::HeaderMap) -> Self {
        let mut headers = HashMap::new();
        for (name, value) in axum_headers {
            let name_lower = name.as_str().to_lowercase();
            let value_str = value.to_str().unwrap_or("").to_string();
            headers
                .entry(name_lower)
                .or_insert_with(Vec::new)
                .push(value_str);
        }
        Self { headers }
    }

    /// Get a header value
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/get
    ///
    /// Returns the header value as a string. For non-Set-Cookie headers,
    /// multiple values are combined with commas per RFC 7230. For Set-Cookie,
    /// only the first value is returned (use get_set_cookie() for all cookies).
    ///
    /// # Arguments
    ///
    /// * `name` - The header name (case-insensitive)
    ///
    /// # Returns
    ///
    /// `Some(value)` if found, `None` otherwise
    pub fn get(&self, name: &str) -> Option<String> {
        let name_lower = name.to_lowercase();
        self.headers.get(&name_lower).map(|values| {
            if name_lower == "set-cookie" {
                // D-08: Set-Cookie stays separate, return first value
                values.first().cloned().unwrap_or_default()
            } else {
                // D-08: Other headers combine with commas
                values.join(", ")
            }
        })
    }

    /// Get all Set-Cookie header values
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/getSetCookie
    ///
    /// Returns all Set-Cookie values as separate strings. This is necessary
    /// because browsers don't combine multiple Set-Cookie headers with commas.
    ///
    /// # Returns
    ///
    /// A vector of all Set-Cookie values (empty if none)
    pub fn get_set_cookie(&self) -> Vec<String> {
        // D-08: Returns all Set-Cookie values separately
        self.headers.get("set-cookie").cloned().unwrap_or_default()
    }

    /// Check if a header exists
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/has
    ///
    /// # Arguments
    ///
    /// * `name` - The header name (case-insensitive)
    ///
    /// # Returns
    ///
    /// `true` if the header exists, `false` otherwise
    pub fn has(&self, name: &str) -> bool {
        self.headers.contains_key(&name.to_lowercase())
    }

    /// Set a header value
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/set
    ///
    /// Replaces any existing values for this header with a single new value.
    /// Header name is normalized to lowercase.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name
    /// * `value` - The header value
    pub fn set(&mut self, name: &str, value: &str) {
        // Replace any existing values
        self.headers
            .insert(name.to_lowercase(), vec![value.to_string()]);
    }

    /// Append a header value
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/append
    ///
    /// Adds a new value for the header, preserving existing values.
    /// Header name is normalized to lowercase.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name
    /// * `value` - The header value to append
    pub fn append(&mut self, name: &str, value: &str) {
        self.headers
            .entry(name.to_lowercase())
            .or_default()
            .push(value.to_string());
    }

    /// Delete a header
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/delete
    ///
    /// Removes all values for the given header name.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name to delete (case-insensitive)
    pub fn delete(&mut self, name: &str) {
        self.headers.remove(&name.to_lowercase());
    }

    /// Iterate over all headers
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Headers/forEach
    ///
    /// Calls the callback for each header. For Set-Cookie, only the first
    /// value is passed; for other headers with multiple values, they are
    /// comma-combined.
    ///
    /// # Arguments
    ///
    /// * `callback` - Function called with (name, value) for each header
    pub fn for_each<F>(&self, callback: F)
    where
        F: FnMut(&str, &str),
    {
        let mut callback = callback;
        for (name, values) in &self.headers {
            let value = if name == "set-cookie" {
                values.first().map(String::as_str).unwrap_or("")
            } else {
                &values.join(", ")
            };
            callback(name, value);
        }
    }

    /// Iterate over all header entries
    ///
    /// Returns an iterator over (name, values) pairs where values is a
    /// vector of all values for that header.
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.headers.iter()
    }

    /// Convert to axum HeaderMap
    ///
    /// Converts NanoHeaders back to axum's HeaderMap format.
    /// All values are added, preserving multiple values for headers
    /// like Set-Cookie.
    ///
    /// # Returns
    ///
    /// An axum `HeaderMap` with all headers
    pub fn to_axum_headers(&self) -> axum::http::HeaderMap {
        let mut map = axum::http::HeaderMap::new();
        for (name, values) in &self.headers {
            for value in values {
                if let Ok(name) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
                    if let Ok(value) = axum::http::HeaderValue::from_str(value) {
                        map.append(name, value);
                    }
                }
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_insensitive_headers() {
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");

        // D-07: Case-insensitive lookup
        assert_eq!(
            headers.get("content-type"),
            Some("application/json".to_string())
        );
        assert_eq!(
            headers.get("CONTENT-TYPE"),
            Some("application/json".to_string())
        );
        assert_eq!(
            headers.get("Content-Type"),
            Some("application/json".to_string())
        );
    }

    #[test]
    fn test_multiple_values() {
        let mut headers = NanoHeaders::new();
        headers.append("Accept", "application/json");
        headers.append("Accept", "text/html");

        // D-08: Non-Set-Cookie headers combine with commas
        assert_eq!(
            headers.get("Accept"),
            Some("application/json, text/html".to_string())
        );
    }

    #[test]
    fn test_set_cookie_separate() {
        let mut headers = NanoHeaders::new();
        headers.append("Set-Cookie", "session=abc; HttpOnly");
        headers.append("Set-Cookie", "user=xyz; Secure");

        // D-08: Set-Cookie values stay separate
        let cookies = headers.get_set_cookie();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0], "session=abc; HttpOnly");
        assert_eq!(cookies[1], "user=xyz; Secure");

        // get() returns first value only for Set-Cookie
        assert_eq!(
            headers.get("Set-Cookie"),
            Some("session=abc; HttpOnly".to_string())
        );
    }

    #[test]
    fn test_from_axum_headers() {
        let mut axum_headers = axum::http::HeaderMap::new();
        axum_headers.insert("X-Custom-Header", "value".parse().unwrap());

        let headers = NanoHeaders::from_axum_headers(&axum_headers);
        assert!(headers.has("x-custom-header"));
        assert_eq!(headers.get("X-Custom-Header"), Some("value".to_string()));
    }

    #[test]
    fn test_headers_api_compliance() {
        let mut headers = NanoHeaders::new();

        // Test all WinterCG Headers methods
        headers.append("Accept", "application/json");
        headers.append("Accept", "text/html");
        headers.set("Content-Type", "application/json");
        headers.append("Set-Cookie", "session=abc");
        headers.append("Set-Cookie", "user=xyz");

        // get() - returns comma-combined (except Set-Cookie)
        assert_eq!(
            headers.get("Accept"),
            Some("application/json, text/html".to_string())
        );
        assert_eq!(
            headers.get("Content-Type"),
            Some("application/json".to_string())
        );

        // get_set_cookie() - returns array
        let cookies = headers.get_set_cookie();
        assert_eq!(cookies.len(), 2);
        assert!(cookies.contains(&"session=abc".to_string()));

        // has()
        assert!(headers.has("Content-Type"));
        assert!(!headers.has("X-Unknown"));

        // Case insensitive
        assert_eq!(
            headers.get("content-type"),
            Some("application/json".to_string())
        );
        assert_eq!(
            headers.get("CONTENT-TYPE"),
            Some("application/json".to_string())
        );

        // delete()
        headers.delete("Accept");
        assert!(!headers.has("Accept"));
    }

    #[test]
    fn test_for_each_iteration() {
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        headers.append("Accept", "text/html");
        headers.append("Accept", "application/xml");

        let mut collected = Vec::new();
        headers.for_each(|name, value| {
            collected.push((name.to_string(), value.to_string()));
        });

        assert!(collected.iter().any(|(n, _)| n == "content-type"));
        assert!(collected
            .iter()
            .any(|(n, v)| n == "accept" && v.contains("text/html")));
    }

    #[test]
    fn test_to_axum_headers() {
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        headers.append("Set-Cookie", "session=abc");
        headers.append("Set-Cookie", "user=xyz");

        let axum_map = headers.to_axum_headers();

        // Check Content-Type
        assert_eq!(
            axum_map.get("content-type").and_then(|v| v.to_str().ok()),
            Some("application/json")
        );

        // Check both Set-Cookie values are preserved
        let cookie_values: Vec<_> = axum_map
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert_eq!(cookie_values.len(), 2);
        assert!(cookie_values.contains(&"session=abc"));
        assert!(cookie_values.contains(&"user=xyz"));
    }
}
