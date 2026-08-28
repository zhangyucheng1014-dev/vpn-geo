package geoip

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestLocateNormalizesCountry(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"country_code":"jp","latitude":35.6,"longitude":139.7}`))
	}))
	defer server.Close()
	location, err := New(server.URL, time.Second).Locate(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if location.CountryCode != "JP" || location.Latitude != 35.6 {
		t.Fatalf("location = %#v", location)
	}
}

func TestLocateRejectsAPIFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":false,"message":"blocked"}`))
	}))
	defer server.Close()
	if _, err := New(server.URL, time.Second).Locate(context.Background()); err == nil {
		t.Fatal("Locate() succeeded for API failure")
	}
}

func TestLocateRejectsInvalidCoordinates(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"country_code":"JP","latitude":91,"longitude":0}`))
	}))
	defer server.Close()
	if _, err := New(server.URL, time.Second).Locate(context.Background()); err == nil {
		t.Fatal("Locate() accepted invalid coordinates")
	}
}
