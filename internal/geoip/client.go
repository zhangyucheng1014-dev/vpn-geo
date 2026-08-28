// Package geoip obtains the current public-IP country after a VPN disconnect.
package geoip

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

type Location struct {
	CountryCode string
	Latitude    float64
	Longitude   float64
}

type Client struct {
	URL        string
	HTTPClient *http.Client
}

type response struct {
	Success     bool    `json:"success"`
	CountryCode string  `json:"country_code"`
	Latitude    float64 `json:"latitude"`
	Longitude   float64 `json:"longitude"`
	Message     string  `json:"message"`
}

func New(url string, timeout time.Duration) *Client {
	if timeout <= 0 {
		timeout = 10 * time.Second
	}
	transport, ok := http.DefaultTransport.(*http.Transport)
	if !ok {
		return &Client{URL: url, HTTPClient: &http.Client{Timeout: timeout}}
	}
	transport = transport.Clone()
	// GeoIP is queried only after a disconnect. Do not retain an idle TLS
	// connection between rare events just to save a future handshake.
	transport.DisableKeepAlives = true
	transport.MaxIdleConns = 0
	return &Client{URL: url, HTTPClient: &http.Client{Timeout: timeout, Transport: transport}}
}

func (c *Client) Locate(ctx context.Context) (Location, error) {
	if c == nil {
		return Location{}, fmt.Errorf("geoip client is nil")
	}
	httpClient := c.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 10 * time.Second}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.URL, nil)
	if err != nil {
		return Location{}, fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "vpn-geo/1.0")
	resp, err := httpClient.Do(req)
	if err != nil {
		return Location{}, fmt.Errorf("geoip request: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return Location{}, fmt.Errorf("geoip HTTP status %s", resp.Status)
	}
	var result response
	decoder := json.NewDecoder(io.LimitReader(resp.Body, 1024*1024))
	if err := decoder.Decode(&result); err != nil {
		return Location{}, fmt.Errorf("decode geoip response: %w", err)
	}
	if !result.Success {
		return Location{}, fmt.Errorf("geoip API reported failure: %s", result.Message)
	}
	country := strings.ToUpper(strings.TrimSpace(result.CountryCode))
	if len(country) != 2 {
		return Location{}, fmt.Errorf("geoip API returned invalid country code %q", result.CountryCode)
	}
	if result.Latitude < -90 || result.Latitude > 90 || result.Longitude < -180 || result.Longitude > 180 {
		return Location{}, fmt.Errorf("geoip API returned invalid coordinates (%.4f, %.4f)", result.Latitude, result.Longitude)
	}
	return Location{CountryCode: country, Latitude: result.Latitude, Longitude: result.Longitude}, nil
}
