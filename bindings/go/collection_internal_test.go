package grafikr

import "testing"

func TestCollectionProviderRequestQueueIsBounded(t *testing.T) {
	provider := &CollectionProvider{requests: make(chan CollectionRequest, 1)}
	request := CollectionRequest{Revision: "1", Request: "1", Start: 0, Count: 1}
	if !provider.enqueue(request) {
		t.Fatal("first collection request was rejected")
	}
	if provider.enqueue(request) {
		t.Fatal("full collection request queue silently accepted another request")
	}
}
