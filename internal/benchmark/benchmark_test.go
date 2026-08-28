package benchmark

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/geoip"
)

func TestNearestSortsByDistanceAndSkipsIncompleteNodes(t *testing.T) {
	nodes := []config.Node{{Name: "Tokyo", Country: "JP", Latitude: 35.6762, Longitude: 139.6503, TestURL: "https://example.test/a"}, {Name: "Seoul", Country: "KR", Latitude: 37.5665, Longitude: 126.9780, TestURL: "https://example.test/b"}, {Name: "No URL", Latitude: 1, Longitude: 1}}
	items := Nearest(nodes, geoip.Location{Latitude: 35.7, Longitude: 139.7}, 3)
	if len(items) != 2 || items[0].Node.Name != "Tokyo" || items[1].Node.Name != "Seoul" {
		t.Fatalf("nearest = %#v", items)
	}
}

func TestNearestReturnsEmptyForNonPositiveCount(t *testing.T) {
	if got := Nearest([]config.Node{{Country: "JP", Latitude: 1, Longitude: 1, TestURL: "https://example.test"}}, geoip.Location{}, 0); got != nil {
		t.Fatalf("Nearest() = %#v, want nil", got)
	}
}

func TestRunNormalizesUnsafeOptions(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("ok"))
	}))
	defer server.Close()
	candidates := []Result{{Node: config.Node{Name: "node", Country: "JP", TestURL: server.URL}}}
	results := Run(context.Background(), candidates, Options{Samples: 0, Bytes: 0, Parallel: 0})
	if len(results) != 1 || results[0].Err != nil || results[0].Bytes == 0 {
		t.Fatalf("Run() = %#v", results)
	}
}

func TestRunRejectsEmptyResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, _ *http.Request) {}))
	defer server.Close()
	results := Run(context.Background(), []Result{{Node: config.Node{Name: "node", TestURL: server.URL}}}, Options{Samples: 1, Bytes: 1, Parallel: 1})
	if results[0].Err == nil {
		t.Fatal("Run() accepted an empty response")
	}
}

func TestNearestReturnsOneNodePerCountry(t *testing.T) {
	nodes := []config.Node{{Name: "Tokyo A", Country: "JP", Latitude: 35.6, Longitude: 139.7, TestURL: "https://example.test/a"}, {Name: "Tokyo B", Country: "JP", Latitude: 35.7, Longitude: 139.7, TestURL: "https://example.test/b"}, {Name: "Seoul", Country: "KR", Latitude: 37.5, Longitude: 127, TestURL: "https://example.test/c"}}
	items := Nearest(nodes, geoip.Location{Latitude: 35.6, Longitude: 139.7}, 2)
	if len(items) != 2 || items[0].Node.Name != "Tokyo A" || items[1].Node.Name != "Seoul" {
		t.Fatalf("nearest countries = %#v", items)
	}
}
