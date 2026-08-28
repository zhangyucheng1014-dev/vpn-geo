// Package benchmark implements an explicit, read-only nearest-node test command.
package benchmark

import (
	"context"
	"fmt"
	"io"
	"math"
	"net/http"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/geoip"
)

type Result struct {
	Node       config.Node
	DistanceKM float64
	Latency    time.Duration
	Bytes      int64
	Throughput float64
	Err        error
}

type Options struct {
	Candidates int
	Samples    int
	Timeout    time.Duration
	Bytes      int64
	Parallel   int
}

func (o Options) normalized() Options {
	if o.Candidates < 1 {
		o.Candidates = 1
	}
	if o.Samples < 1 {
		o.Samples = 1
	}
	if o.Timeout <= 0 {
		o.Timeout = 10 * time.Second
	}
	if o.Bytes < 1 {
		o.Bytes = 1
	}
	if o.Parallel < 1 {
		o.Parallel = 1
	}
	return o
}

func Nearest(nodes []config.Node, location geoip.Location, count int) []Result {
	if count <= 0 {
		return nil
	}
	items := make([]Result, 0, len(nodes))
	for _, node := range nodes {
		if node.TestURL == "" || (node.Latitude == 0 && node.Longitude == 0) {
			continue
		}
		items = append(items, Result{Node: node, DistanceKM: haversine(location.Latitude, location.Longitude, node.Latitude, node.Longitude)})
	}
	sort.SliceStable(items, func(i, j int) bool { return items[i].DistanceKM < items[j].DistanceKM })
	// candidate_countries means exactly that: choose the geographically nearest
	// configured node for each country, not several nodes in one country.
	nearestCountries := make([]Result, 0, count)
	seenCountries := make(map[string]bool)
	for _, item := range items {
		country := strings.ToUpper(item.Node.Country)
		if seenCountries[country] {
			continue
		}
		seenCountries[country] = true
		nearestCountries = append(nearestCountries, item)
		if len(nearestCountries) == count {
			break
		}
	}
	return nearestCountries
}

func Run(ctx context.Context, candidates []Result, options Options) []Result {
	if len(candidates) == 0 {
		return candidates
	}
	options = options.normalized()
	client := newHTTPClient(options.Timeout)
	sem := make(chan struct{}, options.Parallel)
	var wg sync.WaitGroup
	for i := range candidates {
		wg.Add(1)
		go func(index int) {
			defer wg.Done()
			select {
			case sem <- struct{}{}:
			case <-ctx.Done():
				candidates[index].Err = ctx.Err()
				return
			}
			defer func() { <-sem }()
			candidates[index] = test(ctx, candidates[index], options, client)
		}(i)
	}
	wg.Wait()
	sort.SliceStable(candidates, func(i, j int) bool {
		if candidates[i].Err != nil {
			return false
		}
		if candidates[j].Err != nil {
			return true
		}
		return candidates[i].Throughput > candidates[j].Throughput
	})
	return candidates
}

func newHTTPClient(timeout time.Duration) *http.Client {
	transport, ok := http.DefaultTransport.(*http.Transport)
	if ok {
		transport = transport.Clone()
		transport.DisableKeepAlives = true
		transport.MaxIdleConns = 0
	}
	return &http.Client{Timeout: timeout, Transport: transport}
}

func test(parent context.Context, result Result, options Options, client *http.Client) Result {
	options = options.normalized()
	var totalBytes int64
	var totalDuration time.Duration
	var firstLatency time.Duration
	for sample := 0; sample < options.Samples; sample++ {
		ctx, cancel := context.WithTimeout(parent, options.Timeout)
		start := time.Now()
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, result.Node.TestURL, nil)
		if err == nil {
			req.Header.Set("Range", fmt.Sprintf("bytes=0-%d", options.Bytes-1))
			req.Header.Set("User-Agent", "vpn-geo/1.0")
			resp, requestErr := client.Do(req)
			err = requestErr
			if err == nil {
				if resp.StatusCode < 200 || resp.StatusCode > 299 {
					err = fmt.Errorf("HTTP status %s", resp.Status)
				} else {
					firstLatency = time.Since(start)
					read, readErr := io.Copy(io.Discard, io.LimitReader(resp.Body, options.Bytes))
					totalBytes += read
					if readErr != nil {
						err = readErr
					} else if read == 0 {
						err = io.ErrUnexpectedEOF
					}
				}
				resp.Body.Close()
			}
		}
		elapsed := time.Since(start)
		cancel()
		if err != nil {
			result.Err = err
			return result
		}
		totalDuration += elapsed
	}
	result.Latency = firstLatency
	result.Bytes = totalBytes
	if totalDuration > 0 {
		result.Throughput = float64(totalBytes) / totalDuration.Seconds()
	}
	return result
}

func haversine(lat1, lon1, lat2, lon2 float64) float64 {
	const earthKM = 6371.0
	dLat, dLon := radians(lat2-lat1), radians(lon2-lon1)
	a := math.Sin(dLat/2)*math.Sin(dLat/2) + math.Cos(radians(lat1))*math.Cos(radians(lat2))*math.Sin(dLon/2)*math.Sin(dLon/2)
	return earthKM * 2 * math.Atan2(math.Sqrt(a), math.Sqrt(1-a))
}
func radians(degrees float64) float64 { return degrees * math.Pi / 180 }

func FormatMbps(bytesPerSecond float64) string {
	return fmt.Sprintf("%.2f Mbps", bytesPerSecond*8/1000/1000)
}
func Country(node config.Node) string { return strings.ToUpper(node.Country) }
