# ListVettedPackagesResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**vetted_packages** | Option<[**Vec<models::VettedPackages>**](VettedPackages.md)> | All ``VettedPackages`` that contain at least one ``VettedPackage`` matching both a ``PackageMetadataFilter`` and a ``TopologyStateFilter``. Sorted by synchronizer_id then participant_id. | [optional]
**next_page_token** | **String** | Pagination token to retrieve the next page. Empty string if there are no further results. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


